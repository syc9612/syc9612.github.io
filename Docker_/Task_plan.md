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
- `practices/04-container-pid-namespace.md`
- `practices/05-overlayfs-image-layers.md`
- `practices/06-docker-to-kubernetes.md`
- `practices/07-packet-processing-docker.md`
- `practices/08-docker-security.md`
- `practices/09-mini-docker.md`

## 완료된 섹션

### 1장. Docker network 내부 패킷 흐름

개념 문서:

- Docker 기본 네트워크 구성 요소 정리
- network namespace, veth pair, Linux bridge, iptables/nftables 역할 정리
- 컨테이너에서 외부로 나가는 패킷 흐름 설명
- `eth0`가 veth pair의 컨테이너 쪽 endpoint이고 host 쪽 veth peer가 `docker0`에 붙는다는 표현 보정
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
- `container eth0 <-> peer veth <-> docker0 bridge <-> host routing/NAT <-> host NIC` 구조 그림 추가
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
- `depends_on` 짧은 문법과 `condition: service_healthy` 관계 보강
- one-shot job에서 `condition: service_completed_successfully`를 쓰는 예시 추가
- 운영에 가까운 검증 checklist

### 4장. Container PID namespace 내부

개념 문서:

- PID namespace 기본 구조와 host PID/container PID 차이 정리
- 컨테이너 내부 PID 1의 의미와 컨테이너 생명주기 연결
- `docker stop`의 signal 전달 흐름 설명
- shell wrapper, exec form, `exec` 사용 기준 정리
- zombie process와 child process 회수 책임 설명
- `docker run --init`, Compose `init: true` 역할 정리
- `docker top`, `docker exec`, `nsenter`, `/proc/<pid>/ns/pid` 관찰 도구 정리
- `--pid=host`, `--pid=container:<id>`의 장단점 설명

실습 문서:

- `practices/04-container-pid-namespace.md`

실습 내용:

- host PID와 container PID 비교
- PID namespace inode 확인
- `docker top`과 `docker exec ps` 관점 차이 확인
- `nsenter`로 PID namespace 진입
- PID 1 종료와 컨테이너 생명주기 확인
- `docker stop`과 `SIGTERM` 처리 확인
- `--init`으로 작은 init 프로세스 확인
- zombie process 관찰
- `--pid=host`, `--pid=container:<id>` 비교
- Troubleshooting과 정리 명령 추가

### 5장. OverlayFS와 이미지 레이어

개념 문서:

- 이미지 layer와 container writable layer 구분
- OverlayFS의 `lowerdir`, `upperdir`, `workdir`, `merged` 역할 정리
- copy-on-write, copy-up, whiteout 흐름 설명
- Dockerfile 명령과 layer/cache 관계 정리
- build context, `.dockerignore`, multi-stage build 기준 정리
- 이지레이어 빌드 이미지에서 runtime image 분리와 layer 최적화 연결

실습 문서:

- `practices/05-overlayfs-image-layers.md`

실습 내용:

- storage driver 확인
- 실습용 Dockerfile 작성과 이미지 빌드
- `docker image history`, `docker image inspect`로 layer 확인
- 소스 변경 후 build cache invalidation 확인
- `docker inspect`로 `GraphDriver.Data` 확인
- `UpperDir`, `MergedDir` 관찰
- `docker diff`로 copy-on-write 변경사항 확인
- 수정, 삭제, 추가 파일과 whiteout 관찰
- volume/bind mount와 writable layer 차이 정리
- 이지레이어 multi-stage build 최적화 기준 추가
- Troubleshooting과 정리 명령 추가

### 6장. Kubernetes까지 연결

개념 문서:

- Docker Compose와 Kubernetes 리소스 대응 정리
- Pod와 workload controller, Deployment, DaemonSet, StatefulSet, Job 역할 구분
- Kubernetes runtime과 CRI, Docker Engine 직접 의존성 차이 설명
- Docker bridge와 Kubernetes CNI, Pod IP, Service, EndpointSlice, kube-proxy data path 비교
- ConfigMap, Secret, Volume, `hostPath` 사용 기준 정리
- Compose `healthcheck`와 Kubernetes `startupProbe`, `readinessProbe`, `livenessProbe` 차이 정리
- 이지레이어를 Kubernetes로 옮길 때 고려할 host network, capability, device plugin, hugepage, DaemonSet 구조 정리
- 공식 Kubernetes 문서 reference 섹션 추가

실습 문서:

- `practices/06-docker-to-kubernetes.md`

실습 내용:

- Compose service와 Kubernetes 리소스 대응 확인
- 실습 namespace 생성
- ConfigMap과 Secret 생성
- `nginx` 기반 control API 샘플 Deployment와 Service 적용
- Pod, Service, EndpointSlice 관찰
- Service DNS와 `kubectl port-forward` 접근 확인
- probe 상태 확인
- 실제 이지레이어 control API manifest로 바꿀 때 수정할 항목 정리
- DaemonSet, `hostNetwork`, `securityContext`, `hostPath` 템플릿 추가
- CNI, kube-proxy, Service data path 확인 명령 추가
- Troubleshooting, 정리 명령, 공식 reference 추가

### 7장. Packet processing 시스템에서 Docker 쓰는 방식

개념 문서:

- control plane과 packet data path 분리 기준 정리
- bridge, host, macvlan, ipvlan, `none` network mode 비교
- Docker bridge NAT/firewall rule과 host/macvlan/ipvlan의 차이 설명
- 일반 socket, raw socket/libpcap, TUN/TAP, AF_XDP, DPDK packet I/O 방식 구분
- capability, device mount, hugepage, VFIO, BPF/XDP 권한 기준 정리
- CPU pinning, NUMA locality, IRQ affinity, RSS/queue, logging 성능 튜닝 축 정리
- 이지레이어 packet I/O 방식 확인 질문과 설계 영향 정리
- 공식 Docker, Linux kernel, DPDK reference 섹션 추가

실습 문서:

- `practices/07-packet-processing-docker.md`

실습 내용:

- 이지레이어 packet I/O 방식 분류 질문 정리
- host NIC, route, CPU, NUMA, IRQ 정보 확인 명령 추가
- bridge network 경로와 Docker NAT/firewall rule 확인
- host network 동작과 Compose 예시 정리
- macvlan/ipvlan 설계 예시와 주의점 정리
- raw socket `NET_RAW` capability 확인 예시 추가
- TUN/TAP, `NET_ADMIN` device/capability 템플릿 추가
- AF_XDP host 점검 항목과 Compose 템플릿 추가
- DPDK hugepage, PCI driver, VFIO, CPU/NUMA 점검 항목 추가
- 이지레이어 Compose 설계안과 Kubernetes DaemonSet 연결 예시 추가
- Troubleshooting, 정리 명령, 공식 reference 추가

### 8장. Docker 보안

개념 문서:

- Docker 기본 보안 모델과 daemon/socket 권한 구조 정리
- capability 최소화와 `cap_drop: [ALL]`에서 필요한 권한만 더하는 기준 정리
- seccomp 기본 profile, `seccomp=unconfined` 사용 주의 정리
- AppArmor와 SELinux 역할, profile/label 차이 정리
- user namespace remap, rootless Docker, non-root container user 차이 정리
- Docker socket mount, `privileged: true`, host root filesystem mount 위험 정리
- 이지레이어 control API, raw capture, AF_XDP/eBPF, DPDK/VFIO별 보안 설계 기준 정리
- hardened Compose 예시와 공식 reference 섹션 추가

실습 문서:

- `practices/08-docker-security.md`

실습 내용:

- Docker daemon security options 확인
- 컨테이너 내부 root와 `--user` non-root 실행 비교
- `/proc/self/status`로 capability와 seccomp 상태 확인
- raw socket `NET_RAW` capability 최소 부여 예시 추가
- seccomp 기본 profile과 `seccomp=unconfined` 비교 주의점 정리
- AppArmor `docker-default` profile 확인
- SELinux `:z`, `:Z` bind mount label 기준 정리
- read-only root filesystem과 `tmpfs` 예시 추가
- Docker socket mount 위험 점검 명령 추가
- `privileged: true` 대안 템플릿 추가
- rootless와 userns-remap 확인 명령 정리
- 이지레이어 hardened Compose 템플릿과 Kubernetes `securityContext` 예시 추가
- Troubleshooting, 정리 명령, 공식 reference 추가

### 9장. 직접 미니 Docker 만들기

개념 문서:

- 학습용 미니 Docker와 실제 Docker의 범위 차이 정리
- UTS, PID, mount, network, IPC, user, cgroup namespace 역할 정리
- `unshare`, `chroot`, `pivot_root`, `/proc` mount 관계 정리
- cgroup v2의 `cgroup.controllers`, `cgroup.subtree_control`, `cgroup.procs`, `memory.max`, `cpu.max`, `pids.max` 역할 설명
- veth pair, Linux bridge, IP/route, NAT 구성 요소 정리
- 학습용 미니 Docker 흐름과 실제 Docker가 추가로 제공하는 기능 정리
- 공식 Linux man-pages와 kernel cgroup v2 reference 섹션 추가

실습 문서:

- `practices/09-mini-docker.md`

실습 내용:

- 실습 디렉터리와 Alpine rootfs 준비
- `chroot`로 rootfs 확인
- UTS namespace hostname 격리 확인
- user namespace UID/GID mapping 확인
- PID namespace와 `/proc` mount 확인
- `chroot`와 PID/mount namespace 조합 실행
- mount namespace와 tmpfs mount 격리 확인
- cgroup v2 선택 실습과 `pids.max`, `memory.max` 예시 추가
- network namespace, veth pair, Linux bridge 직접 구성
- rootfs를 network namespace 안에서 실행
- `pivot_root` 개념과 `chroot` 차이 정리
- Docker가 추가로 자동화하는 기능 정리
- Troubleshooting, 정리 명령, 공식 reference 추가

## 남은 작업

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
- 이후 신규 장에는 참고한 공식 reference 섹션을 추가한다.

### 기술 정확성 개선

- iptables/nftables 차이는 계속 명시한다.
- Docker Desktop과 Linux Docker의 차이를 네트워크 실습마다 반복해서 주의시킨다.
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
- 5장 OverlayFS 실습은 실제 Linux Docker 환경에서 `UpperDir`, whiteout 출력 예시를 검증한 뒤 보강한다.
- 6장 Kubernetes 실습은 실제 cluster에서 `kubectl apply`, EndpointSlice, probe 출력 예시를 검증한 뒤 보강한다.
- 7장 packet processing 실습은 실제 Linux NIC 환경에서 bridge/host/macvlan/ipvlan, raw socket, DPDK/AF_XDP 가능 여부를 검증한 뒤 보강한다.
- 8장 보안 실습은 실제 Linux Docker 환경에서 seccomp/AppArmor/SELinux/rootless 출력 예시를 검증한 뒤 보강한다.
- 9장 미니 Docker 실습은 disposable Linux VM에서 namespace, cgroup, veth/bridge 절차를 검증한 뒤 출력 예시를 보강한다.

## 다음 작업 우선순위

1. 이지레이어의 실제 빌드 방식 확인 후 Compose 예시 구체화
2. 5장 OverlayFS 실습을 실제 Linux Docker 환경에서 검증하고 출력 예시 보강
3. 6장 Kubernetes 실습을 실제 cluster에서 검증하고 출력 예시 보강
4. 7장 packet processing 실습을 실제 Linux NIC 환경에서 검증하고 출력 예시 보강
5. 8장 보안 실습을 실제 Linux Docker 환경에서 검증하고 출력 예시 보강
6. 9장 미니 Docker 실습을 disposable Linux VM에서 검증하고 출력 예시 보강
7. 10장 `Docker Desktop vs Linux Docker 차이` 작성
