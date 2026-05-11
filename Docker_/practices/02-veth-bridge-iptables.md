# 실습 02. veth + bridge + iptables 실제 생성 구조

이 실습은 컨테이너 하나를 실행했을 때 Docker가 실제로 어떤 network namespace, veth, bridge 연결, iptables 규칙을 만드는지 추적한다.

## 목표

- 컨테이너의 PID와 network namespace를 확인한다.
- 컨테이너 내부 `eth0`와 host 쪽 veth peer를 연결해서 찾는다.
- host veth가 `docker0` bridge에 붙어 있는지 확인한다.
- 포트 매핑이 만든 DNAT 규칙을 확인한다.
- 외부 통신을 위한 MASQUERADE 규칙을 확인한다.

## 전제

Linux Docker 환경을 기준으로 한다.

Docker Desktop for Windows/macOS에서는 실제 Linux 네트워크 장치와 iptables 규칙이 내부 VM 안에 있으므로 host OS에서 그대로 보이지 않을 수 있다. WSL2 또는 Linux VM에서 실습하는 편이 좋다.

명령 중 일부는 root 권한이 필요하다. 필요하면 `sudo`를 붙인다.

## 1. 테스트 컨테이너 실행

```bash
docker run -d --name web-veth-lab -p 8080:80 nginx
```

확인:

```bash
docker ps --filter name=web-veth-lab
```

컨테이너 IP 확인:

```bash
docker inspect -f '{{range.NetworkSettings.Networks}}{{.IPAddress}}{{end}}' web-veth-lab
```

예상 형태:

```text
172.17.0.2
```

## 2. 컨테이너 PID 확인

host에서 컨테이너의 실제 PID를 확인한다.

```bash
docker inspect -f '{{.State.Pid}}' web-veth-lab
```

예:

```text
12345
```

이 PID는 host PID namespace에서 보이는 nginx 컨테이너 프로세스의 PID다.

환경 변수로 저장해두면 이후 명령이 편하다.

```bash
PID=$(docker inspect -f '{{.State.Pid}}' web-veth-lab)
echo $PID
```

PowerShell에서는 다음처럼 저장한다.

```powershell
$PID_VALUE = docker inspect -f '{{.State.Pid}}' web-veth-lab
$PID_VALUE
```

## 3. 컨테이너 network namespace 진입

Linux에서 `nsenter`를 사용하면 컨테이너의 network namespace 안에서 명령을 실행할 수 있다.

```bash
nsenter -t "$PID" -n ip addr
nsenter -t "$PID" -n ip route
```

확인할 내용:

```text
eth0@ifXX
inet 172.17.0.x/16
default via 172.17.0.1 dev eth0
```

`eth0@ifXX`의 `XX`는 host 쪽 peer interface index다.

## 4. 컨테이너 eth0의 peer_ifindex 확인

컨테이너 namespace 안에서 `eth0`의 ifindex와 peer_ifindex를 확인한다.

```bash
nsenter -t "$PID" -n cat /sys/class/net/eth0/ifindex
nsenter -t "$PID" -n cat /sys/class/net/eth0/iflink
```

의미:

| 파일 | 의미 |
| --- | --- |
| `ifindex` | 현재 namespace에서 `eth0` 자신의 interface index |
| `iflink` | veth peer의 interface index |

`iflink` 값이 host namespace에 있는 veth의 ifindex와 일치하면 두 장치가 veth pair라는 뜻이다.

## 5. host 쪽 veth 찾기

host에서 veth 목록을 확인한다.

```bash
ip -o link show type veth
```

출력 예:

```text
18: vethabcd@if17: <BROADCAST,MULTICAST,UP,LOWER_UP> ...
```

앞의 `18`은 host veth 자신의 ifindex이고, `@if17`은 peer 쪽 interface index다.

컨테이너에서 확인한 `eth0`의 `iflink` 값과 host veth의 ifindex를 맞춰보면 된다.

좀 더 직접 찾고 싶다면 host에서 다음처럼 확인한다.

```bash
for i in /sys/class/net/veth*/ifindex; do echo "$i: $(cat "$i")"; done
```

컨테이너 `eth0`의 `iflink` 값과 같은 숫자를 가진 veth가 host 쪽 peer다.

## 6. veth가 docker0에 붙었는지 확인

```bash
bridge link
```

출력에서 host veth가 `docker0`에 붙어 있는지 확인한다.

예상 형태:

```text
vethabcd master docker0
```

또는 다음 명령으로 `docker0`의 연결 상태를 본다.

```bash
ip link show master docker0
```

의미:

```text
host veth
  -> master docker0
  -> docker0 bridge의 포트로 동작
```

## 7. docker0 bridge 주소 확인

```bash
ip addr show docker0
```

예상 형태:

```text
inet 172.17.0.1/16
```

컨테이너 내부 default route의 gateway와 같은 값인지 확인한다.

```bash
nsenter -t "$PID" -n ip route
```

연결해서 보면 다음 구조가 된다.

```text
container eth0 172.17.0.2/16
  -> default via 172.17.0.1
  -> docker0 172.17.0.1/16
```

## 8. iptables NAT 규칙 확인

Docker가 만든 NAT 규칙을 확인한다.

```bash
iptables -t nat -S
```

외부로 나가는 트래픽을 위한 규칙:

```text
-A POSTROUTING ... -j MASQUERADE
```

포트 매핑을 위한 규칙:

```text
-A DOCKER ... -p tcp ... --dport 8080 -j DNAT --to-destination 172.17.0.x:80
```

의미:

```text
container -> external
  source NAT by MASQUERADE

host:8080 -> container:80
  destination NAT by DNAT
```

규칙을 사람이 읽기 쉬운 형태로 보려면 다음 명령도 쓸 수 있다.

```bash
iptables -t nat -L -n -v
```

## 9. nftables backend 확인

iptables가 legacy인지 nft backend인지 확인한다.

```bash
iptables -V
```

예:

```text
iptables v1.8.x (nf_tables)
```

`nf_tables`라고 나오면 iptables 명령을 쓰고 있어도 실제 backend는 nftables다.

nftables ruleset을 직접 보려면 다음을 사용한다.

```bash
nft list ruleset
```

이 명령은 출력이 길 수 있다. Docker 관련 chain, NAT, DNAT, masquerade 키워드를 중심으로 보면 된다.

## 10. 패킷 흐름 최종 정리

컨테이너에서 외부로 나가는 흐름:

```text
web-veth-lab process
  -> eth0 inside container namespace
  -> veth peer in host namespace
  -> docker0 bridge
  -> host routing
  -> iptables MASQUERADE
  -> host NIC
```

host 포트로 들어오는 흐름:

```text
client
  -> HOST_IP:8080
  -> iptables DNAT
  -> 172.17.0.x:80
  -> docker0
  -> host veth
  -> container eth0
  -> nginx
```

## 11. 정리

```bash
docker rm -f web-veth-lab
```

필요하면 사용하지 않는 Docker 리소스를 확인한다.

```bash
docker system df
```
