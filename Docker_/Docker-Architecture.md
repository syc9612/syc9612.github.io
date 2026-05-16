# Docker 기본 구조 정리

이 문서는 Docker를 단순한 실행 명령 모음이 아니라 Linux 커널 기능 위에 만들어진 컨테이너 실행 플랫폼으로 이해하기 위한 개념 문서이다.

실제 명령 실행, 출력 확인, 실습 절차는 `practices/` 아래의 주제별 Markdown 파일에서 다룬다. 이 파일은 각 주제의 구조, 핵심 개념, 설계 판단 기준을 중심으로 유지한다.

## 문서 구성 원칙

| 구분 | 위치 | 역할 |
| --- | --- | --- |
| 개념 문서 | `Docker-Architecture.md` | Docker 내부 구조와 판단 기준 정리 |
| 실습 문서 | `practices/*.md` | 명령 실행, 결과 해석, 검증 절차 정리 |
| 작업 계획 | `Task_plan.md` | 진행 기록, 남은 작업, 개선점 관리 |

## 목차

1. [Docker network 내부 패킷 흐름](#1-docker-network-내부-패킷-흐름)
2. [veth + bridge + iptables 실제 생성 구조](#2-veth--bridge--iptables-실제-생성-구조)
3. [Docker Compose 실전](#3-docker-compose-실전)
4. [Container PID namespace 내부](#4-container-pid-namespace-내부)
5. [OverlayFS와 이미지 레이어](#5-overlayfs와-이미지-레이어)
6. [Kubernetes까지 연결](#6-kubernetes까지-연결)
7. [Packet processing 시스템에서 Docker 쓰는 방식](#7-packet-processing-시스템에서-docker-쓰는-방식)
8. [Docker 보안: capability, seccomp, rootless](#8-docker-보안-capability-seccomp-rootless)
9. [직접 미니 Docker 만들기: unshare, chroot, cgroup](#9-직접-미니-docker-만들기-unshare-chroot-cgroup)
10. [Docker Desktop vs Linux Docker 차이](#10-docker-desktop-vs-linux-docker-차이)

---

## 1. Docker network 내부 패킷 흐름

실습 문서: [01-docker-network-packet-flow.md](./practices/01-docker-network-packet-flow.md)

Docker 컨테이너는 기본적으로 독립된 network namespace 안에서 실행된다. 컨테이너 내부에서 보이는 `eth0`는 물리 NIC가 아니라 host namespace에 있는 veth peer와 연결된 가상 인터페이스다.

Docker 기본 bridge 네트워크는 다음 구성 요소의 조합이다.

| 구성 요소 | 개념적 역할 |
| --- | --- |
| network namespace | 컨테이너마다 독립된 네트워크 스택을 제공한다. |
| veth pair | container namespace와 host namespace를 연결하는 가상 케이블이다. |
| Linux bridge | 여러 컨테이너 veth를 같은 L2 네트워크로 묶는다. |
| routing table | 컨테이너 트래픽을 bridge 또는 외부 NIC 방향으로 보낸다. |
| iptables/nftables | NAT, 포트 포워딩, 필터링을 적용한다. |
| conntrack | NAT 변환 전후의 연결 상태를 추적한다. |

기본 bridge 네트워크의 외부 송신 흐름은 다음 구조로 이해할 수 있다.

```text
container process
  -> container eth0
  -> veth pair
  -> docker0 bridge
  -> host routing
  -> source NAT
  -> host NIC
  -> external network
```

외부에서 컨테이너로 들어오는 포트 매핑 흐름은 반대 방향이지만, 핵심은 destination NAT다.

```text
external client
  -> host published port
  -> destination NAT
  -> docker0 bridge
  -> veth pair
  -> container eth0
  -> container process
```

같은 bridge network에 있는 컨테이너끼리는 host의 물리 NIC를 거치지 않는다. bridge가 MAC 주소 기반으로 veth 사이의 프레임을 전달한다. 사용자 정의 bridge network에서는 Docker 내장 DNS가 컨테이너 이름 또는 서비스 이름을 IP로 해석해준다.

`host network` 모드는 별도 network namespace를 생략하고 host 네트워크 스택을 공유한다. 이 방식은 경로가 단순하고 네트워크 오버헤드가 줄어들 수 있지만, 포트 충돌 가능성과 격리 약화가 생긴다.

### 개념 판단 기준

| 질문 | 판단 |
| --- | --- |
| 컨테이너 간 서비스 통신이 필요한가 | 사용자 정의 bridge network가 기본 선택이다. |
| host NIC의 실제 패킷 경로를 봐야 하는가 | host network 또는 별도 device 접근을 검토한다. |
| 외부에 포트를 공개해야 하는가 | 포트 매핑은 DNAT를 만든다는 점을 고려한다. |
| 성능보다 격리가 중요한가 | bridge network가 다루기 쉽다. |
| NAT 경로가 문제인가 | host, macvlan, ipvlan, DPDK, AF_XDP 계열을 검토한다. |

Docker는 패킷을 직접 처리하는 별도 네트워크 스택을 제공하는 것이 아니다. Linux 커널의 namespace, veth, bridge, routing, netfilter 기능을 조합해 컨테이너 네트워크를 만든다.

---

## 2. veth + bridge + iptables 실제 생성 구조

실습 문서: [02-veth-bridge-iptables.md](./practices/02-veth-bridge-iptables.md)

컨테이너가 기본 bridge 네트워크에 붙을 때 Docker는 여러 커널 리소스를 함께 만든다.

| 리소스 | 의미 |
| --- | --- |
| container network namespace | 컨테이너 전용 네트워크 공간 |
| container `eth0` | 컨테이너 내부에서 보이는 인터페이스 |
| host veth peer | host namespace에 남아 bridge에 붙는 인터페이스 |
| `docker0` bridge | 기본 bridge network의 L2 스위치 역할 |
| IPAM 할당 | 컨테이너 IP와 gateway 구성 |
| NAT 규칙 | 외부 송신과 포트 공개를 위한 주소 변환 |

veth pair는 두 namespace 사이를 연결한다. 한쪽 끝은 컨테이너 안에서 `eth0`로 보이고, 다른 한쪽 끝은 host에서 `docker0` bridge의 포트로 동작한다.

```text
container namespace
  eth0
    |
    | veth pair
    |
host namespace
  vethXXXX
    |
    | bridge port
    |
  docker0
```

Docker가 컨테이너 네트워크를 구성하는 흐름은 개념적으로 다음 순서다.

1. 컨테이너용 network namespace를 준비한다.
2. veth pair를 만든다.
3. veth 한쪽 끝을 컨테이너 namespace로 이동시킨다.
4. 컨테이너 내부 인터페이스 이름을 `eth0`로 맞춘다.
5. host 쪽 veth를 bridge에 연결한다.
6. 컨테이너 IP, gateway, route를 설정한다.
7. 필요 시 NAT와 필터링 규칙을 추가한다.

### bridge와 NAT의 역할 분리

`docker0` bridge는 L2 forwarding을 담당한다. 같은 bridge에 붙은 컨테이너 사이의 트래픽은 MAC 주소 기반으로 전달된다.

iptables/nftables는 L3/L4 경계에서 NAT와 필터링을 담당한다. 기본 bridge 네트워크에서 중요한 NAT는 두 종류다.

| NAT 종류 | 방향 | 목적 |
| --- | --- | --- |
| source NAT, MASQUERADE | container -> external | 컨테이너 사설 IP를 host IP로 변환 |
| destination NAT, DNAT | host published port -> container | host 공개 포트를 컨테이너 IP/포트로 변환 |

이 둘을 구분해야 Docker 네트워크 문제를 분석할 때 어디를 봐야 하는지 명확해진다. bridge 문제는 L2 연결과 veth 상태에 가깝고, 포트 공개 문제는 NAT와 conntrack 상태에 가깝다.

### iptables와 nftables

현대 Linux 배포판에서는 `iptables` 명령을 사용하더라도 내부 backend가 nftables인 경우가 많다. 개념적으로는 netfilter hook에 규칙이 걸린다는 점이 중요하다.

문서에서는 익숙한 용어인 `iptables`를 사용하지만, 실제 환경에서는 legacy iptables와 nftables backend 차이를 확인해야 한다. 이 차이는 명령 출력 형식과 디버깅 위치에 영향을 줄 수 있다.

---

## 3. Docker Compose 실전

실습 문서: [03-docker-compose-easylayer.md](./practices/03-docker-compose-easylayer.md)

Docker Compose는 여러 컨테이너, 네트워크, 볼륨, 환경 변수, 빌드 설정을 하나의 프로젝트 단위로 선언하는 도구다. 단일 컨테이너 실행보다 중요한 점은 “서비스 묶음의 재현성”이다.

Compose는 다음 역할을 담당한다.

| 항목 | 개념적 의미 |
| --- | --- |
| service | 하나의 실행 단위 또는 역할 |
| build | Dockerfile 기반 이미지 생성 과정 |
| image | 실행할 이미지와 배포 단위 |
| network | 서비스 간 통신 범위와 DNS |
| volume | 상태, 설정, 로그, 소스 코드의 외부화 |
| environment | 이미지 밖에서 주입하는 런타임 설정 |
| healthcheck | 프로세스 실행 여부가 아니라 서비스 준비 상태 표현 |
| depends_on | 시작 순서 의존성 표현 |

`depends_on`의 짧은 문법은 컨테이너 생성과 시작 순서를 제어할 뿐, 의존 서비스가 실제 요청을 처리할 준비가 됐다는 뜻은 아니다. readiness가 중요한 서비스는 의존 대상에 `healthcheck`를 정의하고, 의존하는 서비스에서 `condition: service_healthy`를 명시해야 한다.

그래도 `depends_on`은 애플리케이션 레벨의 retry, timeout, reconnect 로직을 대체하지 않는다. Compose가 시작 순서를 도와줄 수는 있지만, 네트워크 지연, DB migration, control API 초기화, 외부 장치 준비 같은 런타임 문제는 애플리케이션이 견딜 수 있어야 한다.

Compose를 사용하면 프로젝트 전용 bridge network가 생성되고, 서비스 이름이 DNS 이름처럼 동작한다. 이 구조는 로컬 개발, 통합 테스트, 단일 host 실험에 적합하다.

### 이지레이어 빌드 관점

이지레이어 같은 C++ 기반 네트워크/패킷 처리 서비스를 Compose로 다룰 때는 일반 웹 애플리케이션보다 빌드 재현성, 네트워크 경로, 권한 모델, 성능 배치가 더 중요하다.

| 고려 항목 | 개념적 기준 |
| --- | --- |
| 빌드 재현성 | compiler, libc, packet library, kernel header 의존성을 고정해야 한다. |
| multi-stage build | 빌드 환경과 런타임 환경을 분리해 이미지 크기와 공격면을 줄인다. |
| 설정 분리 | 룰, 파이프라인, 포트, 로그 경로는 이미지보다 config mount나 env로 관리한다. |
| network mode | control plane과 packet path를 같은 네트워크 모델로 묶을 필요는 없다. |
| capability | 필요한 커널 권한만 명시하고 `privileged` 사용은 최소화한다. |
| device 접근 | NIC, tun/tap, hugepage, AF_XDP, DPDK 요구사항을 host 설정과 함께 본다. |
| CPU 배치 | packet path는 CPU pinning, NUMA, IRQ affinity 영향을 크게 받을 수 있다. |
| 관측성 | 로그, metrics, pcap, debug output이 컨테이너 밖으로 나와야 한다. |

개발 환경에서는 빠른 반복을 위해 bind mount와 debug build가 유용하다. 운영에 가까운 검증에서는 이미지 내부 산출물을 고정하고 설정만 외부화하는 방향이 재현성이 높다.

### 네트워크 설계 기준

| 방식 | 성격 | 적합한 경우 |
| --- | --- | --- |
| bridge network | 기본 격리와 DNS 제공 | control API, 테스트 서비스, 일반 TCP/UDP 서비스 |
| host network | host 네트워크 스택 공유 | 패킷 경로 단순화, 포트 매핑 제거, 성능 실험 |
| macvlan/ipvlan | 컨테이너를 독립 L2/L3 엔드포인트처럼 노출 | 장비처럼 보이는 네트워크 구성이 필요할 때 |
| device 기반 접근 | 특정 커널 장치 또는 NIC 직접 사용 | DPDK, AF_XDP, tun/tap, packet capture |

Compose는 단일 host 환경을 명확하게 구성하는 데 강하다. 여러 host 배포, 자동 복구, rolling update, service discovery, secret 관리가 필요하면 Kubernetes로 전환하는 것이 자연스럽다.

---

## 4. Container PID namespace 내부

실습 문서: [04-container-pid-namespace.md](./practices/04-container-pid-namespace.md)

컨테이너는 host와 분리된 PID namespace 안에서 실행될 수 있다. PID namespace는 프로세스 ID 공간을 분리한다. 같은 프로세스라도 host PID namespace에서 보이는 PID와 컨테이너 내부에서 보이는 PID가 다를 수 있다.

```text
host PID namespace
  process: host PID 12345

container PID namespace
  same process: PID 1
```

Docker에서 `docker inspect -f '{{.State.Pid}}' <container>`로 확인하는 PID는 host PID namespace 기준의 PID다. 반대로 컨테이너 안에서 `ps`를 실행하면 컨테이너 PID namespace 기준의 PID가 보인다.

PID namespace는 계층 구조다. host는 하위 PID namespace의 프로세스를 볼 수 있지만, 컨테이너 내부에서는 host의 전체 프로세스 목록이 보이지 않는다. 이 구조가 컨테이너 프로세스 격리의 핵심이다.

### 컨테이너 PID 1의 의미

컨테이너 내부의 첫 번째 프로세스는 PID 1이 된다. 이 프로세스는 단순히 번호가 1인 프로세스가 아니라 해당 PID namespace의 init 프로세스 역할을 한다.

| 책임 | 설명 |
| --- | --- |
| 컨테이너 생명주기 | PID 1이 종료되면 컨테이너도 종료된다. |
| signal 처리 | `SIGTERM`, `SIGINT` 같은 종료 신호를 애플리케이션 로직에 맞게 처리해야 한다. |
| child process 회수 | 종료된 child process를 `wait` 계열 호출로 회수해야 zombie가 남지 않는다. |
| orphan process 수용 | 부모가 먼저 종료된 child process가 PID 1로 재부모화될 수 있다. |

Docker에서 `docker stop`을 실행하면 기본적으로 PID 1에 `SIGTERM`을 보내고, 제한 시간 안에 종료되지 않으면 `SIGKILL`을 보낸다. 따라서 PID 1이 신호를 제대로 처리하지 않으면 정상 종료 로직, 로그 flush, connection drain, 임시 파일 정리 같은 작업이 실행되지 않을 수 있다.

특히 shell wrapper가 PID 1이 되는 형태는 주의해야 한다.

```dockerfile
CMD sh -c "my-server --config /etc/app/config.yaml"
```

이런 형태에서는 shell이 PID 1이 되고 실제 애플리케이션은 child process가 된다. shell이 종료 신호를 애플리케이션에 전달하지 않거나 child process를 회수하지 않으면 종료 지연 또는 zombie process 문제가 생길 수 있다.

가능하면 다음 방향을 우선한다.

| 방식 | 판단 |
| --- | --- |
| exec form 사용 | `CMD ["my-server", "--config", "/etc/app/config.yaml"]`처럼 애플리케이션이 직접 PID 1이 된다. |
| shell script에서 `exec` 사용 | wrapper가 필요하면 마지막 실행은 `exec my-server ...`로 교체한다. |
| 작은 init 사용 | child process 회수와 signal forwarding이 필요하면 `docker run --init` 또는 Compose `init: true`를 쓴다. |

### zombie process와 init

zombie process는 종료됐지만 부모가 종료 상태를 회수하지 않아 process table에 남아 있는 상태다. 일반 Linux 시스템에서는 init 계열 프로세스가 orphan process를 회수한다. 컨테이너에서는 이 역할을 컨테이너 PID 1이 맡는다.

`docker run --init`을 사용하면 Docker가 작은 init 프로세스를 PID 1로 넣는다. 이 init은 신호 전달과 orphan child 회수에 도움을 준다. 다만 애플리케이션이 직접 만든 child process를 계속 회수하지 않는 버그까지 자동으로 고쳐주는 것은 아니다. 애플리케이션 자체의 signal handling과 child process 관리도 여전히 중요하다.

Compose에서는 다음처럼 같은 효과를 줄 수 있다.

```yaml
services:
  app:
    image: app:local
    init: true
```

### 관찰 도구

PID namespace를 분석할 때 자주 쓰는 도구는 다음과 같다.

| 도구 | 용도 |
| --- | --- |
| `docker top <container>` | host 기준에서 컨테이너 프로세스 목록을 본다. |
| `docker inspect -f '{{.State.Pid}}'` | 컨테이너의 host PID를 확인한다. |
| `docker exec <container> ps` | 컨테이너 내부 PID namespace 기준으로 프로세스를 본다. |
| `nsenter -t <pid> -p` | host에서 특정 프로세스의 PID namespace로 진입한다. |
| `/proc/<pid>/ns/pid` | 프로세스가 속한 PID namespace inode를 확인한다. |

`nsenter`로 PID namespace에 들어가 process tree를 정확히 보려면 mount namespace와 `/proc` mount까지 함께 고려해야 한다. PID namespace만 바꾸고 host의 `/proc`를 그대로 보면 출력이 혼동될 수 있다.

### PID namespace mode 선택

Docker의 기본값은 컨테이너별 private PID namespace다. 필요하면 다른 모드도 선택할 수 있다.

| 방식 | 의미 | 사용 기준 |
| --- | --- | --- |
| 기본값 | 컨테이너마다 별도 PID namespace 사용 | 일반 애플리케이션 기본 선택 |
| `--pid=host` | host PID namespace 공유 | host 프로세스 관찰, low-level debugging, 일부 monitoring agent |
| `--pid=container:<id>` | 다른 컨테이너의 PID namespace 공유 | sidecar 디버깅, 같은 Pod와 유사한 실험 |

`--pid=host`는 컨테이너에서 host의 프로세스 목록을 볼 수 있게 하므로 격리가 약해진다. 운영 환경에서는 모니터링 에이전트처럼 이유가 명확한 경우에만 사용해야 한다.

### 개념 판단 기준

| 질문 | 판단 |
| --- | --- |
| 컨테이너가 종료 신호를 받고 정상 종료해야 하는가 | PID 1의 signal handling을 반드시 확인한다. |
| wrapper shell이 필요한가 | 마지막 실행은 `exec`로 교체하는지 확인한다. |
| child process를 생성하는가 | zombie 회수 책임이 어디에 있는지 확인한다. |
| 여러 프로세스를 한 컨테이너에 넣는가 | `--init` 또는 process supervisor 필요성을 검토한다. |
| host process 관찰이 필요한가 | `--pid=host`의 격리 약화를 감수할 이유가 있는지 기록한다. |

---

## 5. OverlayFS와 이미지 레이어

실습 문서: [05-overlayfs-image-layers.md](./practices/05-overlayfs-image-layers.md)

Docker 이미지는 여러 read-only layer의 합성이다. 컨테이너가 실행되면 그 위에 container writable layer가 추가된다. 이미지 자체는 바뀌지 않고, 컨테이너 실행 중 생긴 변경사항은 writable layer에 기록된다.

```text
container writable layer
image layer N
image layer N-1
base image layer
```

OverlayFS는 여러 디렉터리를 하나의 파일 시스템처럼 보이게 만드는 union filesystem이다. Docker 환경에서는 전통적으로 `overlay2` storage driver에서 이 구조를 관찰할 수 있고, 최신 Docker/containerd 조합에서는 `overlayfs` snapshotter처럼 구현 이름과 host 경로가 달라질 수 있다. 학습 관점에서는 `lowerdir`, `upperdir`, `workdir`, `merged`의 역할을 이해하는 것이 핵심이다.

| 개념 | 의미 |
| --- | --- |
| `lowerdir` | 읽기 전용 이미지 레이어 묶음 |
| `upperdir` | 컨테이너 writable layer 또는 빌드 중 쓰기 가능 레이어 |
| `workdir` | OverlayFS가 내부 작업에 사용하는 디렉터리 |
| `merged` | 컨테이너 프로세스가 보는 합성 파일 시스템 |

### 읽기, 쓰기, 삭제 흐름

컨테이너에서 파일을 읽을 때 OverlayFS는 먼저 `upperdir`을 보고, 없으면 `lowerdir`의 이미지 레이어에서 찾는다. 따라서 컨테이너는 하나의 파일 시스템을 보는 것처럼 느끼지만 실제 데이터는 여러 레이어에 나뉘어 있다.

컨테이너가 기존 이미지 레이어의 파일을 처음 수정하면 Docker는 해당 파일을 writable layer로 복사한 뒤 수정한다. 이를 copy-on-write라고 한다. OverlayFS의 copy-up은 파일 단위로 동작하므로, 큰 파일의 일부만 바꿔도 첫 수정 시 전체 파일 복사가 발생할 수 있다.

파일 삭제도 lower layer를 직접 지우는 방식이 아니다. 이미지 layer는 read-only이므로, writable layer에 whiteout 정보를 남겨 컨테이너 관점에서 해당 파일이 사라진 것처럼 보이게 한다.

### Dockerfile과 layer

Dockerfile의 파일 시스템 변경은 이미지 layer와 빌드 캐시에 직접 영향을 준다. 특히 `RUN`, `COPY`, `ADD`처럼 파일 시스템을 바꾸는 명령은 layer를 만들고, 앞쪽 layer가 바뀌면 뒤쪽 layer의 cache도 다시 계산된다.

| 작성 방식 | 영향 |
| --- | --- |
| 자주 바뀌지 않는 의존성 설치를 앞에 둔다 | 소스 변경 시 package install cache를 재사용하기 쉽다. |
| 자주 바뀌는 소스 `COPY`를 뒤에 둔다 | 코드 수정이 불필요하게 앞쪽 layer를 깨지 않는다. |
| `.dockerignore`를 관리한다 | build context가 작아지고 cache invalidation 범위가 줄어든다. |
| 빌드 산출물만 runtime image로 복사한다 | 이미지 크기와 공격면이 줄어든다. |
| 로그와 상태 파일은 volume으로 뺀다 | container writable layer가 커지는 것을 막는다. |

이지레이어처럼 C++ 빌드 산출물이 큰 프로젝트는 multi-stage build를 적극적으로 사용하는 것이 좋다. 빌드 도구, header, static library, test artifact를 runtime image에서 제거하면 배포 이미지가 단순해진다.

### 운영 판단 기준

| 질문 | 판단 |
| --- | --- |
| 컨테이너에서 계속 증가하는 파일이 있는가 | 로그, pcap, DB 파일은 volume 또는 bind mount로 분리한다. |
| 큰 파일을 런타임에 자주 수정하는가 | copy-up 비용을 피하도록 파일 배치를 조정한다. |
| 빌드가 소스 수정마다 너무 오래 걸리는가 | Dockerfile layer 순서와 build context를 확인한다. |
| 운영 이미지에 빌드 도구가 남아 있는가 | multi-stage build로 runtime image를 분리한다. |
| Docker Desktop에서 host 경로를 확인하는가 | OverlayFS 경로는 내부 Linux VM 기준이라는 점을 고려한다. |

OverlayFS는 컨테이너 파일 시스템을 효율적으로 합성하는 장치이지, 지속 데이터 저장소가 아니다. 컨테이너 삭제와 함께 사라지면 안 되는 데이터는 volume, bind mount, 외부 저장소로 분리해야 한다.

---

## 6. Kubernetes까지 연결

실습 문서: [06-docker-to-kubernetes.md](./practices/06-docker-to-kubernetes.md)

Docker에서 배운 개념은 Kubernetes에서도 대부분 이어진다. 차이는 Kubernetes가 단일 host에서 컨테이너를 실행하는 도구가 아니라, 여러 노드에 걸쳐 원하는 상태를 선언하고 control plane이 그 상태에 맞게 조정하는 오케스트레이션 시스템이라는 점이다.

Docker Compose는 “이 host에서 이 컨테이너 묶음을 어떻게 띄울 것인가”에 가깝고, Kubernetes는 “클러스터가 어떤 상태를 유지해야 하는가”에 가깝다. 그래서 Compose 파일을 Kubernetes manifest로 옮기는 일은 단순 필드 변환이 아니라 workload, network, storage, config, security 모델을 다시 배치하는 작업이다.

| Docker/Compose | Kubernetes |
| --- | --- |
| Docker image | Container image |
| Container | Pod 안의 container |
| Compose service | Deployment, DaemonSet, StatefulSet, Job |
| Compose project network | Pod network, Service, NetworkPolicy |
| Published port | Service `ClusterIP`, `NodePort`, `LoadBalancer`, Ingress/Gateway |
| Bind mount / named volume | Volume, PersistentVolumeClaim, `hostPath`, `emptyDir` |
| `.env`, `environment` | Env, ConfigMap, Secret |
| `healthcheck` | `readinessProbe`, `livenessProbe`, `startupProbe` |
| `restart`, `depends_on` | Controller reconciliation, probe, init container, Job |
| `cap_add`, `devices`, `privileged` | `securityContext`, device plugin, resource request |

### Pod와 workload controller

Kubernetes의 최소 배포 단위는 컨테이너가 아니라 Pod다. Pod는 하나 이상의 컨테이너가 storage와 network context를 공유하는 논리 host다. 같은 Pod 안의 컨테이너들은 같은 IP와 port space를 공유하므로 `localhost`로 통신할 수 있다.

일반적으로 Pod를 직접 운영 단위로 만들기보다는 controller를 사용한다.

| 리소스 | 적합한 경우 |
| --- | --- |
| Deployment | stateless control API, 일반 서버, rolling update가 필요한 서비스 |
| StatefulSet | stable network identity와 persistent storage가 필요한 stateful 서비스 |
| DaemonSet | 모든 노드 또는 특정 노드마다 하나씩 떠야 하는 node-local agent |
| Job | migration, config 검증, batch 작업처럼 완료가 목표인 one-shot 작업 |

Deployment는 Pod와 ReplicaSet의 선언적 update를 관리한다. Compose service 하나를 Kubernetes로 옮길 때 기본 후보는 Deployment지만, host NIC, node-local file, packet path처럼 “어느 노드에서 실행되는가”가 중요하면 DaemonSet을 먼저 검토해야 한다.

### Runtime과 image

Kubernetes는 container image를 실행하지만 Docker Engine에 직접 묶여 있지는 않다. 현대 Kubernetes는 kubelet이 Container Runtime Interface, CRI를 통해 containerd, CRI-O, Docker Engine용 adapter 같은 runtime과 통신한다.

따라서 이식의 핵심은 Docker 명령 자체가 아니라 image와 runtime 요구사항이다.

| 질문 | 판단 |
| --- | --- |
| image가 registry에서 pull 가능한가 | Kubernetes 노드는 local Docker build 결과를 자동으로 공유하지 않는다. |
| runtime image에 필요한 library가 들어 있는가 | Compose에서 host에 기대던 파일이 image에 없는지 확인한다. |
| config가 image에 박혀 있는가 | ConfigMap, Secret, volume mount로 분리한다. |
| Docker socket에 의존하는가 | Kubernetes에서는 강한 권한 경로이므로 대안을 우선 검토한다. |

### Network 모델

Docker 기본 bridge는 한 host 안의 bridge와 NAT 중심으로 이해한다. Kubernetes는 CNI plugin이 구현하는 cluster-wide Pod network를 전제로 한다. 일반적인 Kubernetes network model에서는 Pod마다 cluster 내부에서 고유한 IP가 있고, Pod 간 통신은 같은 노드든 다른 노드든 직접 가능해야 한다.

Kubernetes Service는 변하는 Pod IP 앞에 안정적인 endpoint를 제공한다. Service selector는 label이 맞는 Pod들을 찾고, EndpointSlice는 현재 backend endpoint 집합을 표현한다. kube-proxy 또는 CNI data path는 이 Service 트래픽을 실제 Pod endpoint로 전달한다.

| 계층 | Docker 관점 | Kubernetes 관점 |
| --- | --- | --- |
| 컨테이너 간 이름 해석 | 사용자 정의 bridge의 Docker DNS | Service DNS, Pod DNS |
| 컨테이너 IP | host-local bridge 대역 | cluster-wide Pod CIDR |
| 외부 공개 | `ports`와 DNAT | Service type, Ingress, Gateway |
| data path | bridge, veth, iptables/nftables | CNI, veth, routing/overlay, kube-proxy, iptables/IPVS/eBPF 구현 |
| host network | `--network host` | Pod `hostNetwork: true` |

Service type 선택은 공개 범위에 따라 달라진다.

| Service type | 의미 | 사용 기준 |
| --- | --- | --- |
| `ClusterIP` | cluster 내부에서만 접근하는 stable IP | 내부 control API, backend 통신 |
| `NodePort` | 각 Node IP의 고정 port로 노출 | lab, bare-metal, 외부 LB 앞단 구성 |
| `LoadBalancer` | cloud/external load balancer와 연동 | cloud 환경의 외부 공개 |
| Headless Service | cluster IP 없이 endpoint DNS 제공 | StatefulSet, 직접 endpoint discovery |

### Config, Secret, Volume

Compose에서 bind mount와 env로 처리하던 설정은 Kubernetes에서 ConfigMap, Secret, Volume으로 분리한다.

| 항목 | 역할 |
| --- | --- |
| ConfigMap | 비밀이 아닌 설정을 env, command argument, file로 주입 |
| Secret | password, token, key 같은 민감 정보를 별도 API object로 관리 |
| `emptyDir` | Pod 생명주기에 묶인 임시 공유 디렉터리 |
| PersistentVolumeClaim | Pod 재생성 후에도 보존해야 하는 storage 요청 |
| `hostPath` | node filesystem 직접 mount, 보안 위험이 크므로 제한적으로 사용 |

Secret은 ConfigMap보다 민감 정보에 맞는 API object지만, 기본적으로 etcd 저장 암호화와 RBAC 설계를 별도로 챙겨야 한다. `hostPath`는 kubelet credential, runtime socket, host file을 노출할 수 있으므로 node-local agent처럼 이유가 명확한 경우에만 사용한다.

### Probe와 lifecycle

Compose `healthcheck`는 Kubernetes에서 probe로 나뉜다.

| probe | 의미 |
| --- | --- |
| `startupProbe` | 느린 초기화가 끝났는지 확인하고, 성공 전까지 다른 probe 간섭을 줄인다. |
| `readinessProbe` | Service endpoint로 traffic을 받아도 되는지 판단한다. |
| `livenessProbe` | 프로세스가 복구 불가능한 상태인지 판단하고 재시작을 유도한다. |

readiness와 liveness를 같은 조건으로 두면 장애 처리 의도가 흐려진다. 예를 들어 외부 DB가 잠깐 느린 상황에서 liveness가 실패해 Pod를 계속 재시작하면 오히려 장애를 키울 수 있다. readiness는 traffic 수신 여부, liveness는 자기 프로세스의 복구 불가능 상태에 맞춰 분리하는 편이 안전하다.

### 이지레이어 전환 기준

이지레이어를 Kubernetes로 옮길 때는 control plane과 packet path를 분리해서 생각한다.

| 요구사항 | Kubernetes 후보 |
| --- | --- |
| 일반 control API | Deployment + ClusterIP Service |
| 노드마다 packet capture 또는 NIC 접근 | DaemonSet |
| host network stack 직접 사용 | `hostNetwork: true`, `dnsPolicy: ClusterFirstWithHostNet` 검토 |
| raw socket 또는 interface 설정 | `securityContext.capabilities.add`에서 `NET_RAW`, `NET_ADMIN` 등 최소 부여 |
| NIC, FPGA, GPU, SR-IOV 같은 장치 | device plugin 또는 vendor plugin |
| hugepage, CPU 고정, NUMA 배치 | resource request/limit, hugepage resource, CPU Manager, Topology Manager |
| node-local config/log 접근 | read-only `hostPath` 또는 별도 collector 구조 |

Kubernetes에서 `privileged: true`와 wide-open `hostPath`는 Compose에서보다 더 위험하다. 같은 manifest가 여러 노드에 반복 배포될 수 있고, ServiceAccount/RBAC와 결합되면 cluster 권한 문제로 커질 수 있기 때문이다. 필요한 capability, device, mount, resource를 기능 단위로 좁혀 기록해야 한다.

### 개념 판단 기준

| 질문 | 판단 |
| --- | --- |
| 여러 replica가 같은 방식으로 떠도 되는가 | Deployment가 기본 후보 |
| 특정 node마다 하나씩 떠야 하는가 | DaemonSet을 검토 |
| Pod IP가 바뀌어도 client가 안정적으로 접근해야 하는가 | Service가 필요 |
| config 변경이 image rebuild 없이 가능해야 하는가 | ConfigMap/Secret/Volume mount로 분리 |
| packet path가 host NIC에 직접 묶이는가 | hostNetwork, device plugin, DaemonSet, securityContext를 함께 설계 |
| 성능 튜닝이 CPU/NUMA/device와 연결되는가 | Kubernetes resource manager와 node 설정까지 같이 본다 |

### 참고 reference

- [Kubernetes Pods](https://kubernetes.io/docs/concepts/workloads/pods/)
- [Kubernetes Deployments](https://kubernetes.io/docs/concepts/workloads/controllers/deployment/)
- [Kubernetes Services](https://kubernetes.io/docs/concepts/services-networking/service/)
- [Kubernetes Services, Load Balancing, and Networking](https://kubernetes.io/docs/concepts/services-networking/)
- [Kubernetes Network Plugins](https://kubernetes.io/docs/concepts/extend-kubernetes/compute-storage-net/network-plugins/)
- [Kubernetes ConfigMaps](https://kubernetes.io/docs/concepts/configuration/configmap/)
- [Kubernetes Secrets](https://kubernetes.io/docs/concepts/configuration/secret/)
- [Kubernetes Volumes](https://kubernetes.io/docs/concepts/storage/volumes/)
- [Kubernetes Liveness, Readiness, and Startup Probes](https://kubernetes.io/docs/concepts/configuration/liveness-readiness-startup-probes/)
- [Kubernetes DaemonSet](https://kubernetes.io/docs/concepts/workloads/controllers/daemonset/)
- [Kubernetes Device Plugins](https://kubernetes.io/docs/concepts/extend-kubernetes/compute-storage-net/device-plugins/)
- [Kubernetes Resource Managers](https://kubernetes.io/docs/concepts/workloads/resource-managers/)
- [Kubernetes Security Context](https://kubernetes.io/docs/tasks/configure-pod-container/security-context/)
- [Kubernetes Container Runtimes](https://kubernetes.io/docs/setup/production-environment/container-runtimes/)

---

## 7. Packet processing 시스템에서 Docker 쓰는 방식

실습 문서: [07-packet-processing-docker.md](./practices/07-packet-processing-docker.md)

패킷 처리 시스템에서 Docker를 사용할 때는 일반 웹 서비스보다 네트워크 성능, NIC 접근, 커널 기능, 권한 모델이 더 중요하다.

핵심은 control plane과 packet data path를 분리해서 설계하는 것이다. 관리 API, metrics, health endpoint는 Docker bridge network나 Kubernetes Service로 충분한 경우가 많다. 반대로 실제 packet RX/TX 경로는 host NIC, driver, queue, IRQ, NUMA, hugepage, capability와 직접 연결될 수 있다.

### 네트워크 모드 선택

| 방식 | 성격 | 주요 trade-off |
| --- | --- | --- |
| bridge network | Docker 기본 네트워크 | 관리가 쉽고 DNS/포트 매핑이 편하지만 bridge, veth, NAT 경로가 추가된다. |
| host network | host network namespace 공유 | 경로가 단순하고 port mapping이 없어지지만 격리가 약해지고 port 충돌이 생긴다. |
| macvlan | 컨테이너가 물리망의 별도 MAC을 가진 장비처럼 보임 | L2 설계가 명확해야 하고, host와 macvlan 컨테이너 직접 통신 제약이 있다. |
| ipvlan | parent NIC의 MAC을 공유하고 IP 단위로 분기 | switch MAC table 부담은 줄지만 L2/L3 mode와 routing 설계를 이해해야 한다. |
| `none` + 직접 구성 | Docker 기본 네트워크를 쓰지 않음 | 실험 자유도는 높지만 route, veth, namespace 구성을 직접 책임진다. |

Docker bridge는 control API, 관리 plane, 일반 TCP/UDP 서비스에 좋은 기본값이다. packet path 자체를 실험할 때는 bridge의 NAT와 veth 경로가 측정 결과에 섞일 수 있으므로 host, macvlan, ipvlan, 또는 device 기반 접근을 별도로 검토한다.

Docker는 bridge network에는 firewall/NAT 규칙을 만들지만, host, macvlan, ipvlan network에는 같은 방식의 Docker firewall rule을 만들지 않는다. 이 차이는 보안 정책과 packet trace 위치를 정할 때 중요하다.

### Packet I/O 방식

패킷 처리 애플리케이션이 실제로 어느 API를 쓰는지가 Docker 설계를 결정한다.

| 방식 | kernel stack 사용 | 컨테이너 설계 포인트 |
| --- | --- | --- |
| 일반 TCP/UDP socket | 사용 | bridge 또는 host network 선택, port 공개, conntrack 영향 검토 |
| raw socket / AF_PACKET / libpcap | 사용 | `NET_RAW`, interface visibility, capture 권한, promiscuous mode 검토 |
| TUN/TAP | 사용 | `/dev/net/tun`, `NET_ADMIN`, route 설정 권한 검토 |
| AF_XDP | 일부 우회 | XDP program load 권한, NIC driver/queue 지원, UMEM/ring, zero-copy 지원 확인 |
| DPDK | kernel network stack 우회 가능 | hugepage, VFIO/UIO, PCI device binding, CPU/NUMA 배치, device mount 검토 |

일반 socket과 raw socket은 Linux kernel network stack 위에서 동작한다. 컨테이너가 bridge network에 있으면 container namespace의 인터페이스만 보이고, host network를 쓰면 host의 인터페이스를 그대로 본다.

AF_XDP는 XDP와 user-space socket을 연결해 고성능 packet path를 만들 수 있다. 그러나 NIC driver, queue, XDP mode, kernel version, BPF 권한에 영향을 받으므로 “컨테이너에서 실행된다”는 사실보다 “host에서 XDP가 제대로 동작하는가”가 먼저다.

DPDK는 EAL, hugepage, PCI device, driver binding, CPU core 배치가 핵심이다. 컨테이너는 실행 포장에 가깝고, 실제 성능 조건은 host BIOS, kernel boot option, NIC driver, IOMMU/VFIO, NUMA topology에 의해 결정된다.

### 권한과 장치

권한은 기능 요구사항에서 역으로 도출해야 한다.

| 필요 작업 | 검토 항목 |
| --- | --- |
| raw socket 또는 packet socket 열기 | `NET_RAW` |
| interface, route, qdisc, XDP attach 조작 | `NET_ADMIN` |
| hugepage, locked memory | `IPC_LOCK`, `ulimits.memlock`, `/dev/hugepages` mount |
| TUN/TAP 사용 | `/dev/net/tun` device mount, `NET_ADMIN` |
| VFIO 기반 PCI device 접근 | `/dev/vfio/*`, IOMMU group, device cgroup rule |
| eBPF/XDP program load | kernel version에 따라 `CAP_BPF`, `CAP_NET_ADMIN`, `CAP_PERFMON` 등 검토 |

`privileged: true`는 디버깅을 빠르게 만들지만, host device와 capability를 과하게 열기 때문에 운영 설계의 기본값으로 두면 안 된다. 최소 capability, 필요한 device, read-only mount, 명확한 resource limit을 조합하는 편이 낫다.

### 성능 튜닝 축

packet processing에서는 컨테이너 옵션만으로 성능을 보장할 수 없다. host와 애플리케이션의 배치를 함께 봐야 한다.

| 축 | 확인할 내용 |
| --- | --- |
| CPU pinning | Docker `cpuset`, DPDK `--lcores`, thread affinity가 서로 충돌하지 않는지 확인 |
| NUMA locality | NIC가 붙은 NUMA node와 worker core, memory allocation 위치를 맞춤 |
| IRQ affinity | NIC queue interrupt가 worker core 또는 housekeeping core로 의도대로 배치되는지 확인 |
| RSS / queue | NIC queue 수, RSS hash, worker thread 수가 맞는지 확인 |
| hugepage | DPDK/고성능 allocator가 요구하는 hugepage 크기와 NUMA별 할당량 확인 |
| logging | hot path에서 동기 로그, pcap dump, stdout 과다 출력이 없는지 확인 |

Docker `cpuset`은 컨테이너 프로세스가 실행 가능한 CPU를 제한한다. DPDK `--lcores`는 애플리케이션 내부 lcore를 물리 CPU에 매핑한다. 둘이 어긋나면 DPDK는 특정 core를 쓰도록 설정됐지만 컨테이너 cgroup이 그 CPU를 허용하지 않는 상황이 생길 수 있다.

IRQ affinity는 컨테이너 내부가 아니라 host의 `/proc/irq/*/smp_affinity_list`와 NIC driver 설정에서 조정한다. packet path가 host NIC interrupt에 민감하면 Compose 파일만 봐서는 충분하지 않다.

### 이지레이어 판단 기준

이지레이어가 어떤 packet I/O 모델을 쓰는지 먼저 확인해야 한다.

| 질문 | 설계 영향 |
| --- | --- |
| 일반 TCP/UDP 서버인가 | bridge network + port mapping 또는 Kubernetes Service가 기본 출발점 |
| raw packet capture가 필요한가 | host network, `NET_RAW`, interface 선택, pcap output 위치 검토 |
| interface 설정이나 route 조작이 필요한가 | `NET_ADMIN` 필요성과 명령 범위 확인 |
| AF_XDP를 쓰는가 | host kernel/NIC/driver/XDP queue 지원, BPF 권한, hostNetwork 여부 확인 |
| DPDK를 쓰는가 | hugepage, VFIO/UIO, PCI binding, `/dev/vfio`, CPU/NUMA 배치 확인 |
| 노드마다 NIC를 붙잡아야 하는가 | Kubernetes에서는 DaemonSet과 device plugin 또는 node selector 검토 |
| control API와 packet path를 분리할 수 있는가 | control API는 bridge/Service, packet path는 host/device 기반으로 나누는 구조 검토 |

현실적인 출발점은 control API와 metrics를 일반 network에 두고, packet path만 host network 또는 device 기반 접근으로 분리하는 것이다. 이 구조는 운영 관측성과 성능 실험을 동시에 다루기 쉽다.

### 참고 reference

- [Docker network drivers](https://docs.docker.com/engine/network/drivers/)
- [Docker bridge network driver](https://docs.docker.com/engine/network/drivers/bridge/)
- [Docker host network driver](https://docs.docker.com/engine/network/drivers/host/)
- [Docker macvlan network driver](https://docs.docker.com/engine/network/drivers/macvlan/)
- [Docker ipvlan network driver](https://docs.docker.com/engine/network/drivers/ipvlan/)
- [Docker packet filtering and firewalls](https://docs.docker.com/engine/network/packet-filtering-firewalls/)
- [Docker run reference: runtime privilege and Linux capabilities](https://docs.docker.com/engine/containers/run/)
- [Docker Compose services reference](https://docs.docker.com/reference/compose-file/services/)
- [Linux kernel AF_XDP documentation](https://docs.kernel.org/networking/af_xdp.html)
- [Linux kernel SMP IRQ affinity](https://www.kernel.org/doc/html/v6.9/core-api/irq/irq-affinity.html)
- [Linux kernel CPU isolation](https://docs.kernel.org/admin-guide/cpu-isolation.html)
- [DPDK Getting Started Guide for Linux](https://doc.dpdk.org/guides/linux_gsg/)
- [DPDK Running Sample Applications](https://doc.dpdk.org/guides/linux_gsg/build_sample_apps.html)
- [DPDK EAL parameters](https://doc.dpdk.org/guides-25.11/linux_gsg/linux_eal_parameters.html)

---

## 8. Docker 보안: capability, seccomp, rootless

실습 문서: [08-docker-security.md](./practices/08-docker-security.md)

Docker 보안은 하나의 기능이 아니라 여러 격리 장치의 조합이다.

| 장치 | 역할 |
| --- | --- |
| namespace | 프로세스, 네트워크, mount, IPC, UTS 등을 분리한다. |
| cgroup | CPU, memory, process 수 같은 리소스를 제한한다. |
| capability | root 권한을 세부 권한으로 나눈다. |
| seccomp | 프로세스가 호출할 수 있는 syscall을 제한한다. |
| AppArmor/SELinux | 파일, capability, process 접근 정책을 강제한다. |
| user namespace | 컨테이너 root와 host root의 매핑을 분리한다. |
| rootless mode | daemon과 컨테이너를 root 권한 없이 실행한다. |

컨테이너 안의 root는 host root와 완전히 같은 의미는 아니지만, 위험한 mount와 capability가 결합되면 host 침해로 이어질 수 있다. 특히 Docker socket mount, `privileged`, host root filesystem mount, 넓은 device mount는 강한 권한 상승 경로가 될 수 있다.

### 기본 보안 모델

Docker는 기본적으로 container process를 host process와 분리한다. 그러나 Docker daemon은 일반적으로 root 권한으로 실행되고, container runtime은 host namespace, cgroup, mount, network, device 설정을 조작한다. 그래서 “컨테이너 내부에서 root가 아니다”와 “Docker daemon을 제어할 수 없다”는 별개 문제다.

Docker socket에 접근할 수 있는 사용자는 Docker daemon에 API 요청을 보낼 수 있다. 이 권한은 host filesystem mount, privileged container 실행 같은 방식으로 host root에 준하는 영향력을 가질 수 있다. `docker` group도 단순 편의 권한이 아니라 root-level 권한으로 취급해야 한다.

### Capability 최소화

Linux capability는 root 권한을 세부 권한으로 나눈 것이다. Docker는 기본 capability 집합을 제공하고, 필요하면 `cap_drop`, `cap_add`로 조정한다.

| 작업 | 필요한 권한 후보 | 주의점 |
| --- | --- | --- |
| raw socket, packet socket | `NET_RAW` | packet capture 또는 ping 같은 기능과 연결 |
| interface, route, qdisc, XDP attach | `NET_ADMIN` | 네트워크 설정 변경 범위가 넓음 |
| hugepage, memory lock | `IPC_LOCK`, `ulimits.memlock` | DPDK/고성능 packet path에서 자주 등장 |
| ptrace/debugging | `SYS_PTRACE` | 운영 기본값으로 열지 않음 |
| mount, namespace, 많은 관리 작업 | `SYS_ADMIN` | 너무 넓어 사실상 작은 `privileged`처럼 취급 |

운영 기본값은 `cap_drop: [ALL]`에서 시작하고 필요한 capability만 다시 더하는 방식이 가장 명확하다. 다만 일부 이미지나 runtime은 기본 capability를 전제로 만들어졌을 수 있으므로 기능 테스트가 필요하다.

### Seccomp

seccomp는 컨테이너 프로세스가 호출할 수 있는 syscall을 제한한다. Docker의 기본 seccomp profile은 allowlist 방식으로 동작하며, 위험하거나 namespacing이 어려운 syscall을 막는다.

기본 profile을 끄는 `seccomp=unconfined`는 문제 원인 확인용으로만 사용하고, 운영 기본값으로 두지 않는다. 특정 syscall이 꼭 필요하다면 해당 기능을 재현하는 최소 테스트를 만들고 custom seccomp profile에 필요한 예외만 추가한다.

### AppArmor와 SELinux

AppArmor와 SELinux는 Linux Security Module, LSM 계층에서 파일, process, capability, mount, network 접근을 더 제한한다. 배포판에 따라 기본 보안 모듈이 다르다.

| 항목 | 특징 |
| --- | --- |
| AppArmor | profile 이름 기반 정책. Docker는 기본적으로 `docker-default` profile을 사용할 수 있다. |
| SELinux | label 기반 정책. volume mount에는 `:z`, `:Z` label 옵션이 영향을 준다. |

LSM은 capability보다 더 세밀하게 파일과 kernel object 접근을 막을 수 있다. 반대로 잘못된 profile이나 label은 정상 volume mount와 device 접근을 막을 수 있으므로, permission denied가 발생하면 capability뿐 아니라 AppArmor/SELinux audit log도 함께 확인한다.

### User namespace와 rootless

user namespace remap은 컨테이너 내부 UID 0을 host의 비특권 UID 범위에 매핑한다. 컨테이너 안에서는 root처럼 보이지만 host에서는 높은 번호의 비특권 UID로 동작한다. 단, `userns-remap`에서는 Docker daemon 자체는 여전히 root로 실행된다.

rootless mode는 Docker daemon과 container를 모두 비root 사용자 namespace 안에서 실행한다. daemon/runtime 취약점의 영향을 줄이는 데 도움이 되지만, network, cgroup, privileged workload, 일부 storage/device 기능에 제약이 있을 수 있다.

| 방식 | 장점 | 제약 |
| --- | --- | --- |
| non-root user in container | 애플리케이션 권한 축소가 단순함 | Docker daemon/root mount 위험은 별도 문제 |
| userns-remap | container root를 host 비특권 UID로 매핑 | daemon은 root, 기존 `/var/lib/docker`와 호환 주의 |
| rootless Docker | daemon과 container 모두 비root 실행 | network/device/cgroup 기능 제약 가능 |

### 위험한 패턴

| 패턴 | 위험 |
| --- | --- |
| `/var/run/docker.sock` mount | 컨테이너가 Docker daemon API를 통해 host 제어 가능 |
| `privileged: true` | 모든 capability, 많은 device, LSM 완화가 결합됨 |
| host root filesystem mount | host 파일 변경과 secret 탈취 가능 |
| broad `hostPath` 또는 bind mount | container escape가 아니어도 host 데이터 손상 가능 |
| `seccomp=unconfined`, `apparmor=unconfined`, `label=disable` | 방어선을 의도적으로 제거 |
| root user + writable rootfs | 침해 후 persistence와 도구 설치가 쉬워짐 |

보안 설계의 기본 방향은 다음과 같다.

| 원칙 | 설명 |
| --- | --- |
| 최소 권한 | 필요한 capability만 추가한다. |
| 읽기 전용 기본값 | 설정 파일은 read-only mount를 우선한다. |
| runtime 분리 | 빌드 도구와 디버깅 도구를 운영 이미지에서 제거한다. |
| syscall 제한 | 기본 seccomp profile을 유지하고 예외만 검토한다. |
| rootless 검토 | 기능 제약을 감수할 수 있으면 rootless가 방어선을 추가한다. |

추가로 운영 이미지에서는 `read_only: true`, `tmpfs`, read-only config mount, non-root `USER`, `no-new-privileges:true`, 로그/pcap output volume 분리를 함께 검토한다.

### 이지레이어 보안 기준

이지레이어처럼 네트워크 권한이 필요한 서비스는 보안과 기능 요구가 충돌할 수 있다. 이 경우 “왜 이 capability가 필요한지”를 기능 단위로 기록해야 운영 검토가 가능하다.

| 기능 요구 | 보안 설계 |
| --- | --- |
| control API만 제공 | non-root user, bridge network, read-only rootfs, default seccomp |
| raw packet capture | `NET_RAW`만 추가 가능한지 먼저 확인 |
| interface 설정 필요 | `NET_ADMIN` 범위를 명령/기능 단위로 기록 |
| AF_XDP/eBPF | kernel capability, BPF mount, hostNetwork 필요성을 분리 검토 |
| DPDK/VFIO | `/dev/vfio/*`, hugepage mount, `IPC_LOCK`, CPU/NUMA 조건을 최소화 |
| debug shell 필요 | 운영 이미지가 아니라 debug profile 또는 별도 debug image로 분리 |

운영 Compose 예시는 다음 방향을 기본값으로 둔다.

```yaml
services:
  easylayer:
    image: easylayer:local
    user: "65532:65532"
    read_only: true
    cap_drop:
      - ALL
    cap_add:
      - NET_RAW
    security_opt:
      - no-new-privileges:true
    volumes:
      - ./config:/etc/easylayer:ro
    tmpfs:
      - /tmp
```

이 예시는 출발점일 뿐이다. 실제 packet I/O 방식에 따라 `NET_ADMIN`, `/dev/net/tun`, `/dev/vfio`, hugepage mount, hostNetwork가 추가될 수 있다. 추가할 때마다 기능 요구와 위험을 함께 기록한다.

### 참고 reference

- [Docker Engine security](https://docs.docker.com/engine/security/)
- [Docker run reference: runtime privilege and Linux capabilities](https://docs.docker.com/engine/containers/run/)
- [Docker seccomp security profiles](https://docs.docker.com/engine/security/seccomp/)
- [Docker AppArmor security profiles](https://docs.docker.com/engine/security/apparmor/)
- [Docker user namespace remap](https://docs.docker.com/engine/security/userns-remap/)
- [Docker rootless mode](https://docs.docker.com/engine/security/rootless/)
- [Docker rootless tips](https://docs.docker.com/engine/security/rootless/tips/)
- [Protect the Docker daemon socket](https://docs.docker.com/engine/security/protect-access/)
- [Docker bind mounts and SELinux labels](https://docs.docker.com/engine/storage/bind-mounts/)
- [Docker Compose services reference](https://docs.docker.com/reference/compose-file/services/)
- [Kubernetes security context](https://kubernetes.io/docs/tasks/configure-pod-container/security-context/)
- [Kubernetes seccomp](https://kubernetes.io/docs/reference/node/seccomp/)

---

## 9. 직접 미니 Docker 만들기: unshare, chroot, cgroup

실습 문서: 예정

Docker의 핵심 아이디어는 Linux 커널 기능을 조합해 프로세스를 격리 실행하는 것이다. 학습용 미니 Docker를 만들면 Docker가 감추고 있는 구성 요소를 직접 확인할 수 있다.

핵심 구성 요소는 다음과 같다.

| 기능 | 역할 |
| --- | --- |
| `unshare` | 새로운 namespace를 만들어 격리된 실행 공간을 만든다. |
| `chroot`/`pivot_root` | 프로세스가 보는 root filesystem을 바꾼다. |
| mount namespace | 컨테이너 전용 mount table을 제공한다. |
| PID namespace | 컨테이너 내부 PID 공간을 분리한다. |
| network namespace | 컨테이너 전용 네트워크 스택을 만든다. |
| cgroup | 리소스 사용량을 제한하고 관찰한다. |
| veth/bridge | 컨테이너 네트워크를 host와 연결한다. |

학습용 미니 Docker의 개념 흐름은 다음과 같다.

```text
rootfs 준비
  -> namespace 생성
  -> root filesystem 전환
  -> proc/sysfs mount
  -> cgroup 제한 적용
  -> network 연결
  -> command 실행
```

실제 Docker는 이보다 훨씬 많은 기능을 제공한다. 이미지 레이어, registry 연동, container lifecycle, logging, restart policy, volume driver, network driver, API server, 보안 profile 등이 모두 필요하다.

---

## 10. Docker Desktop vs Linux Docker 차이

실습 문서: 예정

Linux Docker는 Linux 커널 위에서 Docker daemon과 컨테이너 런타임이 직접 동작한다. 반면 Docker Desktop은 Windows/macOS에서 Linux 컨테이너를 실행하기 위해 내부 Linux VM을 사용한다.

```text
Linux Docker
  host Linux kernel
  dockerd/containerd
  containers

Docker Desktop
  Windows/macOS host
  Docker Desktop
  internal Linux VM
  dockerd/containerd
  containers
```

이 구조 차이는 다음 영역에 영향을 준다.

| 항목 | Linux Docker | Docker Desktop |
| --- | --- | --- |
| Docker daemon 위치 | host Linux | 내부 Linux VM |
| container namespace | host에서 직접 관찰 가능 | 내부 VM 안에 존재 |
| `docker0`, veth | host Linux에 존재 | host OS에 직접 보이지 않을 수 있음 |
| iptables/nftables | host Linux에서 확인 | 내부 VM 기준으로 확인 |
| bind mount | native filesystem | OS와 VM 경계의 파일 공유 |
| host network | Linux 동작 기준 | Desktop 구현 제약 존재 |
| 성능 특성 | 커널과 파일 시스템 직접 사용 | VM/파일 공유 계층 영향 |

네트워크, namespace, OverlayFS 같은 내부 구조를 학습할 때는 Linux VM, WSL2 내부 Linux, 또는 실제 Linux 서버가 더 명확하다. Docker Desktop은 개발 편의성은 높지만 내부 구조 관찰에는 VM 경계를 항상 고려해야 한다.

---

## 다음 작성 방향

이 문서는 개념 중심으로 유지한다. 이후 각 장을 확장할 때는 다음 기준을 따른다.

- 핵심 개념과 구조를 먼저 설명한다.
- 명령 실행 절차는 `practices/` 문서로 분리한다.
- 실습 링크를 각 장 상단에 둔다.
- Docker Desktop과 Linux Docker 차이를 필요한 곳에서 명시한다.
- 이지레이어 관련 내용은 packet path, 빌드 재현성, 권한 모델, 성능 튜닝 관점으로 연결한다.
