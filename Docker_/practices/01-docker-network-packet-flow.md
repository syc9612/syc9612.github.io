# 실습 01. Docker network 내부 패킷 흐름

이 실습은 Docker 기본 bridge 네트워크에서 컨테이너 패킷이 어떤 경로로 이동하는지 확인한다.

## 목표

- 컨테이너의 network namespace와 host network namespace를 구분한다.
- 컨테이너 내부 `eth0`와 host 쪽 veth peer의 관계를 확인한다.
- `docker0` bridge와 기본 route를 확인한다.
- 포트 매핑 시 DNAT 규칙이 어떻게 생기는지 확인한다.
- 사용자 정의 bridge network에서 컨테이너 이름 기반 통신을 확인한다.

## 전제

Linux Docker 환경을 기준으로 한다.

Docker Desktop for Windows/macOS에서는 컨테이너와 Docker daemon이 내부 Linux VM에서 실행되므로, Windows PowerShell 또는 macOS host 터미널에서 `docker0`, veth, iptables가 그대로 보이지 않을 수 있다. 네트워크 구조 관찰은 Linux VM, WSL2 내부, 또는 실제 Linux 서버에서 진행하는 편이 좋다.

## 1. 테스트 컨테이너 실행

```bash
docker run -d --name web -p 8080:80 nginx
```

실행 확인:

```bash
docker ps
```

기대하는 상태:

```text
0.0.0.0:8080->80/tcp
```

host의 `8080` 포트가 컨테이너의 `80` 포트로 연결되어 있으면 된다.

## 2. Docker bridge network 확인

```bash
docker network ls
docker network inspect bridge
```

`docker network inspect bridge`에서 볼 항목:

| 항목 | 의미 |
| --- | --- |
| `Subnet` | 기본 bridge network의 IP 대역 |
| `Gateway` | 컨테이너가 기본 gateway로 사용하는 bridge IP |
| `Containers` | 이 네트워크에 붙어 있는 컨테이너 |
| `IPAddress` | 컨테이너에 할당된 IP |
| `MacAddress` | 컨테이너 인터페이스의 MAC 주소 |

일반적인 예:

```text
Subnet: 172.17.0.0/16
Gateway: 172.17.0.1
Container IP: 172.17.0.2
```

## 3. 컨테이너 내부 네트워크 확인

```bash
docker exec -it web sh
```

컨테이너 안에서 실행:

```bash
ip addr
ip route
cat /etc/resolv.conf
```

확인할 내용:

```text
eth0: 172.17.0.x/16
default via 172.17.0.1 dev eth0
```

이 의미는 컨테이너가 외부로 나가는 모든 기본 트래픽을 `docker0` bridge의 IP인 `172.17.0.1`로 보낸다는 뜻이다.

컨테이너 shell에서 나온다.

```bash
exit
```

## 4. host 쪽 bridge와 veth 확인

host에서 실행:

```bash
ip addr show docker0
ip link show type veth
bridge link
```

`docker0`는 host namespace에 있는 Linux bridge다. 정확히 말하면 컨테이너의 `eth0`는 veth pair의 컨테이너 쪽 endpoint이고, 반대쪽 host veth peer가 `docker0` bridge에 붙는다.

대략적인 구조:

```text
[container namespace]                 [host namespace]

container process
        |
        v
eth0, veth endpoint  <--- veth pair --->  peer veth, vethXXXX
                                                |
                                                v
                                      docker0 bridge
                                      L2 switch 역할
                                                |
                                                v
                                      host routing
                                      netfilter NAT
                                                |
                                                v
                                      host NIC, eth0/ens...
```

한 줄로 줄이면 다음 흐름이다.

```text
container eth0
  <-> peer veth, host veth
  <-> docker0 bridge, L2 switch
  -> host routing + netfilter NAT
  <-> host NIC
```

## 5. 외부로 나가는 패킷의 NAT 확인

host에서 NAT 테이블을 확인한다.

```bash
iptables -t nat -S
```

찾아볼 규칙:

```text
POSTROUTING ... MASQUERADE
```

`MASQUERADE`는 컨테이너의 source IP를 host의 외부 IP로 바꿔서 외부 네트워크로 내보내는 source NAT 규칙이다.

컨테이너 내부:

```text
src = 172.17.0.x
```

외부로 나갈 때:

```text
src = HOST_IP
```

이 변환 덕분에 외부 서버는 `172.17.0.x`라는 사설 Docker bridge IP가 아니라 host IP와 통신한다고 인식한다.

## 6. 포트 매핑 DNAT 확인

`web` 컨테이너를 `-p 8080:80`으로 실행했으므로 host의 NAT 테이블에는 DNAT 규칙이 생긴다.

```bash
iptables -t nat -S
```

찾아볼 규칙:

```text
DNAT ... tcp dpt:8080 ... to:172.17.0.x:80
```

의미:

```text
before DNAT
  dst = HOST_IP:8080

after DNAT
  dst = 172.17.0.x:80
```

동작 확인:

```bash
curl http://localhost:8080
```

nginx 기본 HTML이 응답하면 host 포트에서 컨테이너 포트로 패킷이 정상 전달된 것이다.

## 7. 사용자 정의 bridge network 확인

사용자 정의 bridge network를 만든다.

```bash
docker network create app-net
```

두 컨테이너를 같은 네트워크에 붙인다.

```bash
docker run -d --name api --network app-net nginx
docker run --rm --network app-net curlimages/curl http://api
```

`curl` 컨테이너가 `http://api`로 접근할 수 있으면 Docker 내장 DNS가 `api` 이름을 컨테이너 IP로 해석한 것이다.

확인:

```bash
docker network inspect app-net
```

## 8. host network 모드 비교

Linux 환경에서 host network 모드를 실행한다.

```bash
docker run --rm --network host nginx
```

이 모드에서는 컨테이너가 host network namespace를 공유한다. 별도의 veth, bridge, 포트 매핑 NAT 경로가 줄어든다.

비교:

| 모드 | 경로 | 특징 |
| --- | --- | --- |
| bridge | container eth0 -> host veth peer -> docker0 -> host routing/NAT -> host NIC | 기본 격리 제공 |
| host | process -> host network stack -> host NIC | 경로 단순, 격리 약화 |

## 9. 정리

기본 bridge network의 핵심 흐름:

```text
container process
  -> eth0, veth pair의 컨테이너 쪽 endpoint
  -> host-side veth peer
  -> docker0 bridge
  -> host routing + netfilter NAT
  -> host NIC
```

포트 매핑의 핵심:

```text
HOST_IP:8080
  -> DNAT
  -> CONTAINER_IP:80
```

컨테이너에서 외부로 나갈 때의 핵심:

```text
CONTAINER_IP
  -> MASQUERADE
  -> HOST_IP
```

## 10. 정리 명령

실습이 끝나면 생성한 리소스를 정리한다.

```bash
docker rm -f web api
docker network rm app-net
```
