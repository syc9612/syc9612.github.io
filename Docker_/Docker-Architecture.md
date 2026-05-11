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

실습 문서: 예정

컨테이너는 host와 분리된 PID namespace 안에서 실행될 수 있다. 이때 host에서 보이는 프로세스 ID와 컨테이너 내부에서 보이는 프로세스 ID가 다르다.

```text
host PID namespace
  process: host PID 12345

container PID namespace
  same process: PID 1
```

컨테이너 내부의 첫 번째 프로세스는 PID 1이 된다. PID 1은 일반 프로세스와 다른 책임을 가진다.

| 책임 | 설명 |
| --- | --- |
| signal 처리 | 종료 신호를 애플리케이션에 올바르게 전달해야 한다. |
| child process 회수 | 종료된 child process를 회수하지 않으면 zombie가 남을 수 있다. |
| 컨테이너 생명주기 | PID 1이 종료되면 컨테이너도 종료된다. |

컨테이너에서 애플리케이션 바이너리를 바로 PID 1로 실행할 때는 signal handling과 zombie process 처리가 중요하다. 이를 보완하기 위해 작은 init 프로세스를 넣는 방식이 자주 사용된다.

`host PID namespace`를 공유하면 디버깅에는 편하지만 프로세스 격리가 약해진다. 운영 환경에서는 필요한 이유가 명확할 때만 사용해야 한다.

---

## 5. OverlayFS와 이미지 레이어

실습 문서: 예정

Docker 이미지는 여러 read-only layer의 합성이다. 컨테이너가 실행되면 그 위에 writable layer가 추가된다.

```text
container writable layer
image layer N
image layer N-1
base image layer
```

OverlayFS는 여러 디렉터리를 하나의 파일 시스템처럼 보이게 만든다.

| 개념 | 의미 |
| --- | --- |
| `lowerdir` | 읽기 전용 이미지 레이어 |
| `upperdir` | 컨테이너 또는 빌드 단계의 쓰기 가능 레이어 |
| `workdir` | OverlayFS 내부 작업 디렉터리 |
| `merged` | 사용자에게 보이는 합성 결과 |

컨테이너 안에서 파일을 수정하면 기존 이미지 레이어를 직접 바꾸지 않는다. 변경된 파일은 writable layer에 기록된다. 이를 copy-on-write라고 한다.

이미지 레이어 구조는 Dockerfile 작성 방식과 직접 연결된다. 자주 변하는 파일을 뒤쪽 layer에 두면 빌드 캐시 재사용성이 좋아지고, 불필요한 빌드 도구를 runtime 이미지에 남기지 않으면 이미지 크기와 공격면이 줄어든다.

이지레이어처럼 C++ 빌드 산출물이 큰 프로젝트는 multi-stage build를 적극적으로 사용하는 것이 좋다. 빌드 도구, header, static library, test artifact를 runtime image에서 제거하면 배포 이미지가 단순해진다.

---

## 6. Kubernetes까지 연결

실습 문서: 예정

Docker에서 배운 개념은 Kubernetes에서도 대부분 이어진다. 차이는 Kubernetes가 단일 host의 컨테이너 실행 도구가 아니라 여러 노드에 걸쳐 컨테이너를 배치하고 관리하는 오케스트레이션 시스템이라는 점이다.

| Docker/Compose | Kubernetes |
| --- | --- |
| Container | Container |
| Image | Container image |
| Compose service | Deployment, StatefulSet, DaemonSet |
| Docker network | CNI network |
| Published port | Service, Ingress, NodePort, LoadBalancer |
| Volume | PersistentVolume, PersistentVolumeClaim |
| Environment | Env, ConfigMap, Secret |
| Healthcheck | livenessProbe, readinessProbe, startupProbe |

Kubernetes의 최소 배포 단위는 컨테이너가 아니라 Pod다. 같은 Pod 안의 컨테이너들은 network namespace를 공유하므로 `localhost`를 통해 통신할 수 있다.

Docker bridge 네트워크를 이해하면 Kubernetes의 CNI, Pod IP, Service, kube-proxy, iptables/IPVS/eBPF data path를 이해하기 쉬워진다.

이지레이어를 Kubernetes로 옮길 때는 일반 Deployment보다 DaemonSet, `hostNetwork`, `securityContext`, device plugin, hugepage resource, CPU pinning 같은 항목을 검토해야 할 가능성이 높다.

---

## 7. Packet processing 시스템에서 Docker 쓰는 방식

실습 문서: 예정

패킷 처리 시스템에서 Docker를 사용할 때는 일반 웹 서비스보다 네트워크 성능, NIC 접근, 커널 기능, 권한 모델이 더 중요하다.

대표적인 실행 방식은 다음과 같다.

| 방식 | 성격 | 주요 trade-off |
| --- | --- | --- |
| bridge network | Docker 기본 네트워크 | 관리가 쉽지만 NAT/bridge 경로가 추가된다. |
| host network | host stack 공유 | 경로가 단순하지만 격리가 약해지고 포트 충돌이 생길 수 있다. |
| macvlan/ipvlan | 컨테이너를 별도 네트워크 엔드포인트처럼 노출 | L2/L3 설계가 명확해야 하며 host와 통신 제약이 있을 수 있다. |
| raw socket/libpcap | 커널 네트워크 스택을 사용한 packet I/O | 권한과 성능 한계를 함께 고려해야 한다. |
| AF_XDP | kernel bypass에 가까운 고성능 packet path | kernel, driver, NIC 지원이 필요하다. |
| DPDK | user-space packet processing | hugepage, NIC binding, CPU/NUMA 튜닝이 필요하다. |

이지레이어가 어떤 packet I/O 모델을 쓰는지에 따라 Docker 설계가 달라진다. control API는 bridge network에 두고, packet path만 host network나 device 기반 접근으로 분리하는 구조가 현실적인 출발점이다.

권한은 기능 요구사항에서 역으로 도출해야 한다. 예를 들어 raw socket이 필요하면 `NET_RAW`, 인터페이스 설정이 필요하면 `NET_ADMIN`, hugepage와 memory lock이 필요하면 `IPC_LOCK`을 검토한다. `privileged`는 빠른 디버깅에는 편하지만 운영 설계의 기본값으로 두면 안 된다.

---

## 8. Docker 보안: capability, seccomp, rootless

실습 문서: 예정

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

컨테이너 안의 root는 host root와 완전히 같은 의미는 아니지만, 위험한 mount와 capability가 결합되면 host 침해로 이어질 수 있다. 특히 Docker socket mount, `privileged`, host root filesystem mount는 강한 권한 상승 경로가 될 수 있다.

보안 설계의 기본 방향은 다음과 같다.

| 원칙 | 설명 |
| --- | --- |
| 최소 권한 | 필요한 capability만 추가한다. |
| 읽기 전용 기본값 | 설정 파일은 read-only mount를 우선한다. |
| runtime 분리 | 빌드 도구와 디버깅 도구를 운영 이미지에서 제거한다. |
| syscall 제한 | 기본 seccomp profile을 유지하고 예외만 검토한다. |
| rootless 검토 | 기능 제약을 감수할 수 있으면 rootless가 방어선을 추가한다. |

이지레이어처럼 네트워크 권한이 필요한 서비스는 보안과 기능 요구가 충돌할 수 있다. 이 경우 “왜 이 capability가 필요한지”를 기능 단위로 기록해야 운영 검토가 가능하다.

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