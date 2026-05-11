# Docker 문서 작업 계획 및 기록

작성일: 2026-05-11

## 목적

Docker의 내부 구조를 네트워크, 파일 시스템, 프로세스 격리, 보안, Kubernetes 연결까지 단계적으로 정리한다.

문서는 두 종류로 분리한다.

| 구분 | 파일 위치 | 역할 |
| --- | --- | --- |
| 개념 문서 | `Docker-Architecture.md` | 전체 구조, 핵심 개념, 판단 기준 정리 |
| 실습 문서 | `practices/*.md` | 주제별 명령 실행, 결과 해석, 검증 절차 정리 |

## 현재까지 작성한 내용

### 1. 기본 문서 구조 작성

파일:

- `Docker-Architecture.md`

작성한 목차:

1. Docker network 내부 패킷 흐름
2. veth + bridge + iptables 실제 생성 구조
3. Docker Compose 실전
4. Container PID namespace 내부
5. OverlayFS와 이미지 레이어
6. Kubernetes까지 연결
7. Packet processing 시스템에서 Docker 쓰는 방식
8. Docker 보안: capability, seccomp, rootless
9. 직접 미니 Docker 만들기: unshare, chroot, cgroup
10. Docker Desktop vs Linux Docker 차이

### 2. 실습 문서 분리 원칙 적용

실습 내용은 본문에 과도하게 넣지 않고 `practices/` 하위에 주제별 파일로 분리하기로 정했다.

현재 생성된 실습 파일:

- `practices/01-docker-network-packet-flow.md`
- `practices/02-veth-bridge-iptables.md`
- `practices/03-docker-compose-easylayer.md`

## 완료된 섹션

### 1장. Docker network 내부 패킷 흐름

개념 문서:

- Docker 기본 네트워크 구성 요소 정리
- network namespace, veth pair, Linux bridge, iptables/nftables 역할 정리
- 컨테이너에서 외부로 나가는 패킷 흐름 설명
- 외부에서 컨테이너로 들어오는 포트 매핑 흐름 설명
- 컨테이너 간 통신 구조 설명
- `--network host` 모드와 bridge 모드 차이 설명
- Docker Desktop 환경에서 관찰 시 주의점 추가

실습 문서:

- `practices/01-docker-network-packet-flow.md`

실습 내용:

- `nginx` 컨테이너 실행
- `docker network inspect bridge` 확인
- 컨테이너 내부 `ip addr`, `ip route`, `/etc/resolv.conf` 확인
- host의 `docker0`, veth, bridge link 확인
- `MASQUERADE`, `DNAT` 규칙 확인
- 사용자 정의 bridge network와 Docker DNS 확인
- host network 모드 비교

### 2장. veth + bridge + iptables 실제 생성 구조

개념 문서:

- Docker가 컨테이너 실행 시 생성하는 리소스 정리
- 컨테이너 namespace와 host namespace의 veth 연결 구조 설명
- `eth0@ifXX`, `vethXXXX@ifYY`의 의미 설명
- Docker 네트워크 생성 순서 정리
- `docker0` bridge의 역할 설명
- `MASQUERADE`, `DNAT` 역할 분리
- iptables와 nftables backend 차이 설명

실습 문서:

- `practices/02-veth-bridge-iptables.md`

실습 내용:

- 테스트 컨테이너 실행
- 컨테이너 PID 확인
- `nsenter`로 network namespace 진입
- `/sys/class/net/eth0/ifindex`, `iflink` 확인
- host 쪽 veth peer 찾기
- veth가 `docker0`에 붙었는지 확인
- NAT 규칙 확인
- nftables backend 확인

### 3장. Docker Compose 실전

개념 문서:

- Compose가 관리하는 요소 정리
- `services`, `build`, `image`, `ports`, `networks`, `volumes`, `environment`, `depends_on`, `healthcheck` 역할 설명
- Compose 기본 network와 서비스 이름 기반 DNS 설명
- 이지레이어 빌드 시 고려할 항목 추가
- network mode 선택 기준 정리
- Compose의 한계와 Kubernetes 전환 연결점 설명

실습 문서:

- `practices/03-docker-compose-easylayer.md`

실습 내용:

- 이지레이어 기준 권장 파일 구조
- multi-stage Dockerfile 예시
- 기본 `compose.yaml` 예시
- 개발용 `compose.override.yaml` 예시
- host network 모드 예시
- capability 최소 부여 예시
- device, hugepage 예시
- CPU pinning 고려
- healthcheck 예시
- 운영에 가까운 검증 checklist

## 남은 작업

### 4장. Container PID namespace 내부

예정 내용:

- PID namespace 기본 구조
- 컨테이너 내부 PID 1의 의미
- signal 처리
- zombie process 회수
- `docker run --init`
- `docker top`
- `nsenter -p`
- `--pid=host`의 장단점

예상 실습 파일:

- `practices/04-container-pid-namespace.md`

### 5장. OverlayFS와 이미지 레이어

예정 내용:

- 이미지 layer와 container writable layer
- copy-on-write
- `lowerdir`, `upperdir`, `workdir`, `merged`
- Dockerfile 명령과 layer 관계
- 빌드 캐시와 이미지 크기 최적화
- 이지레이어 빌드 이미지에서 multi-stage build와 layer 최적화 연결

예상 실습 파일:

- `practices/05-overlayfs-image-layers.md`

### 6장. Kubernetes까지 연결

예정 내용:

- Docker Compose와 Kubernetes 리소스 대응
- Pod, Deployment, Service, ConfigMap, Secret, Volume
- Docker bridge와 Kubernetes CNI 비교
- kube-proxy, iptables/IPVS 개념 연결
- 이지레이어를 Kubernetes로 옮길 때 고려할 host network, capability, device plugin, DaemonSet 구조

예상 실습 파일:

- `practices/06-docker-to-kubernetes.md`

### 7장. Packet processing 시스템에서 Docker 쓰는 방식

예정 내용:

- bridge, host, macvlan, ipvlan 비교
- DPDK, AF_XDP, raw socket, pcap 계열 구분
- NIC 접근 방식
- hugepage, CPU pinning, NUMA, IRQ affinity
- 이지레이어에 가장 직접적으로 연결되는 장

예상 실습 파일:

- `practices/07-packet-processing-docker.md`

### 8장. Docker 보안

예정 내용:

- capability 최소화
- seccomp profile
- rootless Docker
- AppArmor/SELinux
- Docker socket mount 위험
- `privileged: true`의 위험과 대안
- 이지레이어에서 필요한 권한을 최소화하는 기준

예상 실습 파일:

- `practices/08-docker-security.md`

### 9장. 직접 미니 Docker 만들기

예정 내용:

- `unshare`
- `chroot` 또는 `pivot_root`
- `/proc` mount
- cgroup 제한
- veth와 bridge 직접 구성
- 최소 컨테이너 런타임 흐름 이해

예상 실습 파일:

- `practices/09-mini-docker.md`

### 10장. Docker Desktop vs Linux Docker 차이

예정 내용:

- Docker Desktop 내부 Linux VM 구조
- WSL2 backend
- `/var/lib/docker` 위치 차이
- network namespace 관찰 차이
- bind mount 성능 차이
- host network 제약
- Linux 실습과 Docker Desktop 실습을 구분하는 기준

예상 실습 파일:

- `practices/10-docker-desktop-vs-linux.md`

## 개선점

### 문서 구조 개선

- 각 장 시작 부분에 실습 문서 링크를 통일된 형식으로 둔다.
- 본문은 개념과 판단 기준 중심으로 유지한다.
- 실제 명령, 실행 결과 예시, 정리 명령은 실습 문서로 분리한다.
- 실습 문서는 `목표 -> 전제 -> 실행 -> 관찰 -> 정리` 순서로 통일한다.

### 기술 정확성 개선

- iptables/nftables 차이는 계속 명시한다.
- Docker Desktop과 Linux Docker의 차이를 네트워크 실습마다 반복해서 주의시킨다.
- `depends_on`은 readiness 보장이 아니라 시작 순서 제어라는 점을 3장 실습에 보강할 필요가 있다.
- `network_mode: host`가 Docker Desktop에서 Linux와 다르게 동작할 수 있다는 점을 추가 설명하면 좋다.
- capability 예시는 실제 필요한 syscall/작업과 연결해서 더 엄격하게 정리할 필요가 있다.

### 이지레이어 관련 개선

- 이지레이어의 실제 빌드 명령, 바이너리 이름, config 경로가 확정되면 3장 실습 예시를 실제 값으로 바꾼다.
- 이지레이어가 사용하는 packet I/O 방식이 bridge, raw socket, AF_XDP, DPDK 중 무엇인지 확인해야 한다.
- 실제 방식에 따라 필요한 capability, device mount, hugepage, host network 여부를 다시 정리한다.
- 성능 검증용 Compose와 개발용 Compose를 분리하는 방향이 좋다.
- 운영 배포까지 고려하면 Compose 이후 Kubernetes의 DaemonSet, hostNetwork, securityContext로 연결해야 한다.

### 실습 품질 개선

- 각 실습마다 예상 출력 예시를 조금 더 추가한다.
- 실패 시 확인할 항목을 `Troubleshooting` 섹션으로 추가한다.
- 실습 종료 후 정리 명령을 모든 실습 문서에 통일한다.
- Linux 전용 명령과 PowerShell 대체 명령을 구분해서 표시한다.
- 실제 Docker 명령을 실행할 수 있는 환경에서 결과를 검증한 뒤 출력 예시를 보강한다.

## 다음 작업 우선순위

1. 4장 `Container PID namespace 내부` 작성
2. `practices/04-container-pid-namespace.md` 생성
3. 3장 실습에 `depends_on`과 healthcheck 관계 보강
4. 5장 `OverlayFS와 이미지 레이어` 작성
5. 이지레이어의 실제 빌드 방식 확인 후 Compose 예시 구체화