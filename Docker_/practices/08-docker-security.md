# 실습 08. Docker 보안: capability, seccomp, rootless

이 실습은 Docker 컨테이너를 운영에 가깝게 실행할 때 확인해야 할 보안 경계를 정리한다. capability, seccomp, AppArmor/SELinux, user namespace, rootless mode, Docker socket mount, `privileged: true`의 위험을 기능 요구사항과 연결해서 본다.

## 목표

- Docker가 기본으로 적용하는 보안 옵션을 확인한다.
- 컨테이너 내부 root와 host root의 차이와 한계를 이해한다.
- capability를 `drop ALL`에서 필요한 것만 추가하는 방식으로 정리한다.
- seccomp, AppArmor, SELinux가 어떤 위치에서 막는지 구분한다.
- Docker socket mount와 `privileged: true`를 운영 기본값에서 제외하는 이유를 확인한다.
- 이지레이어에 필요한 권한을 기능 단위로 기록하는 기준을 만든다.

## 전제

Linux Docker 환경을 기준으로 한다.

Docker Desktop for Windows/macOS에서는 Linux 컨테이너가 내부 VM에서 실행되므로 AppArmor, SELinux, host namespace, Docker daemon socket 위치가 Linux 서버와 다르게 보일 수 있다. 보안 profile 관찰은 실제 Linux 서버, Linux VM, 또는 WSL2 내부 Linux에서 진행하는 편이 명확하다.

일부 명령은 root 권한이 필요하다. 이 문서는 위험한 설정을 실제로 적용하기보다 관찰과 템플릿 작성에 초점을 둔다.

## 1. Docker 보안 옵션 확인

Docker daemon이 어떤 보안 옵션을 쓰는지 확인한다.

```bash
docker info --format '{{json .SecurityOptions}}'
```

일반 출력 예:

```text
["name=apparmor","name=seccomp,profile=builtin","name=cgroupns"]
```

PowerShell에서도 같은 명령을 사용한다.

```powershell
docker info --format '{{json .SecurityOptions}}'
```

확인할 항목:

| 항목 | 의미 |
| --- | --- |
| `seccomp` | syscall filtering 사용 |
| `apparmor` | AppArmor profile 사용 |
| `selinux` | SELinux label 기반 confinement 사용 |
| `rootless` | Docker daemon이 rootless mode로 실행 중 |
| `userns` | user namespace remap 사용 |

실행 중인 컨테이너의 profile도 확인한다.

```bash
docker run -d --name security-lab alpine sleep 1d
docker inspect -f 'AppArmor={{.AppArmorProfile}} SecurityOpt={{json .HostConfig.SecurityOpt}} Privileged={{.HostConfig.Privileged}}' security-lab
```

정리:

```bash
docker rm -f security-lab
```

## 2. 컨테이너 내부 root 확인

기본 컨테이너는 내부에서 root로 실행되는 경우가 많다.

```bash
docker run --rm alpine id
```

예상:

```text
uid=0(root) gid=0(root)
```

non-root 사용자로 실행한다.

```bash
docker run --rm --user 65532:65532 alpine id
```

예상:

```text
uid=65532 gid=65532
```

의미:

| 방식 | 효과 |
| --- | --- |
| container root | container namespace 안에서는 root 권한을 가짐 |
| `--user` non-root | 애플리케이션 프로세스 권한 축소 |
| userns-remap/rootless | host에서 보이는 UID 권한까지 축소 |

`--user`는 좋은 출발점이지만 Docker daemon socket mount, dangerous bind mount, `privileged` 같은 위험을 자동으로 없애주지는 않는다.

## 3. capability 상태 확인

컨테이너의 capability bitmask를 본다.

```bash
docker run --rm alpine sh -c 'grep Cap /proc/self/status'
```

`cap_drop: ALL`과 비교한다.

```bash
docker run --rm --cap-drop ALL alpine sh -c 'grep Cap /proc/self/status'
```

raw socket이 필요한지 테스트한다.

```bash
docker run --rm python:3.12-alpine python -c "import socket; socket.socket(socket.AF_PACKET, socket.SOCK_RAW, socket.htons(3)); print('ok')"
```

권한이 없으면 다음과 비슷하게 실패할 수 있다.

```text
PermissionError: [Errno 1] Operation not permitted
```

`NET_RAW`만 추가해서 다시 확인한다.

```bash
docker run --rm --cap-drop ALL --cap-add NET_RAW python:3.12-alpine python -c "import socket; socket.socket(socket.AF_PACKET, socket.SOCK_RAW, socket.htons(3)); print('ok')"
```

예상:

```text
ok
```

정리:

| 기능 | capability 후보 |
| --- | --- |
| raw socket, AF_PACKET, 일부 packet capture | `NET_RAW` |
| interface, route, qdisc, XDP attach | `NET_ADMIN` |
| memory lock, hugepage 기반 packet path | `IPC_LOCK` |
| process debugging | `SYS_PTRACE` |
| broad host 관리 작업 | `SYS_ADMIN`, 가능하면 피함 |

운영 Compose에서는 다음처럼 출발한다.

```yaml
services:
  easylayer:
    image: easylayer:local
    cap_drop:
      - ALL
    cap_add:
      - NET_RAW
```

`NET_ADMIN`, `IPC_LOCK`, `SYS_PTRACE`는 실제 기능 요구와 테스트 결과가 있을 때만 추가한다.

## 4. seccomp 확인

Docker 기본 seccomp profile은 allowlist 방식이다. 기본 profile이 적용되는지 확인한다.

```bash
docker run --rm alpine sh -c 'grep Seccomp /proc/self/status'
```

예상 형태:

```text
Seccomp:        2
Seccomp_filters:        1
```

의미:

| 값 | 의미 |
| --- | --- |
| `Seccomp: 0` | seccomp 미사용 |
| `Seccomp: 2` | filtering mode 사용 |

`seccomp=unconfined`는 비교용으로만 사용한다.

```bash
docker run --rm --security-opt seccomp=unconfined alpine sh -c 'grep Seccomp /proc/self/status'
```

운영 기준:

- 기본 seccomp profile은 유지한다.
- syscall 문제가 의심되면 `strace`, audit log, 최소 재현 테스트로 필요한 syscall을 확인한다.
- `seccomp=unconfined`는 디버깅 중 원인 분리에만 사용하고 운영 Compose에 남기지 않는다.

## 5. AppArmor 확인

AppArmor가 있는 Linux 환경에서 profile 상태를 본다.

```bash
docker run -d --name apparmor-lab alpine sleep 1d
docker inspect -f '{{.AppArmorProfile}}' apparmor-lab
```

예상:

```text
docker-default
```

host에서 AppArmor 상태를 본다.

```bash
sudo aa-status
```

`aa-status`가 없다면 배포판에 AppArmor 도구가 설치되어 있지 않을 수 있다.

정리:

```bash
docker rm -f apparmor-lab
```

운영 기준:

- 기본 `docker-default` profile을 끄지 않는다.
- custom profile은 특정 파일, network, capability 차단이 필요한 경우에만 작성한다.
- AppArmor denied 로그는 `dmesg` 또는 audit log에서 확인한다.

## 6. SELinux 확인

SELinux 환경인지 확인한다.

```bash
getenforce
```

가능한 출력:

```text
Enforcing
Permissive
Disabled
```

SELinux가 활성화된 host에서 bind mount permission 문제가 생기면 label 옵션을 확인한다.

```bash
docker run --rm -v "$PWD/config:/etc/easylayer:ro,Z" alpine ls /etc/easylayer
```

공유 mount가 필요한 경우에는 `:z`, 특정 컨테이너 전용 label이 필요하면 `:Z`를 검토한다.

주의:

- `:Z`는 host path label을 바꿀 수 있으므로 공용 디렉터리에 함부로 쓰지 않는다.
- `--security-opt label=disable`은 SELinux confinement을 끄므로 운영 기본값으로 두지 않는다.
- SELinux 문제는 capability 추가로 해결되지 않을 수 있다.

## 7. 읽기 전용 root filesystem

운영 컨테이너는 가능한 한 root filesystem을 read-only로 둔다.

실패 예:

```bash
docker run --rm --read-only alpine sh -c 'touch /tmp/test'
```

예상:

```text
Read-only file system
```

쓰기 가능한 임시 디렉터리가 필요하면 `tmpfs`를 붙인다.

```bash
docker run --rm --read-only --tmpfs /tmp alpine sh -c 'touch /tmp/test && ls -l /tmp/test'
```

Compose 예시:

```yaml
services:
  easylayer:
    image: easylayer:local
    read_only: true
    tmpfs:
      - /tmp
    volumes:
      - ./config:/etc/easylayer:ro
      - ./logs:/var/log/easylayer
```

로그, pcap, debug dump는 writable rootfs가 아니라 명시적인 volume으로 분리한다.

## 8. Docker socket mount 위험

다음 형태는 강한 위험 신호다.

```yaml
services:
  manager:
    image: some-manager
    volumes:
      - /var/run/docker.sock:/var/run/docker.sock
```

위 mount가 있으면 컨테이너 안에서 Docker daemon API에 접근할 수 있다. 공격자가 이 컨테이너를 장악하면 새 privileged 컨테이너 실행, host filesystem mount, secret 접근 같은 작업으로 이어질 수 있다.

점검 명령:

```bash
docker ps --format '{{.Names}}' | while read name; do
  docker inspect -f '{{.Name}} {{json .Mounts}}' "$name" | grep docker.sock || true
done
```

PowerShell:

```powershell
docker ps --format '{{.Names}}' | ForEach-Object {
  docker inspect -f '{{.Name}} {{json .Mounts}}' $_ | Select-String 'docker.sock'
}
```

대안:

- Docker socket을 mount하지 않는다.
- 꼭 필요하면 read-only proxy라고 믿기 전에 API method allowlist가 있는 proxy를 검토한다.
- manager 기능은 host systemd service나 별도 제한된 API로 분리한다.
- 원격 daemon 접근은 SSH 또는 TLS로 보호한다.

## 9. `privileged: true` 대안

`privileged: true`는 모든 capability, host device 접근, LSM 완화가 함께 열릴 수 있다. 운영 기본값으로 두지 않는다.

나쁜 출발점:

```yaml
services:
  easylayer:
    image: easylayer:local
    privileged: true
```

대신 기능 요구를 분해한다.

```yaml
services:
  easylayer:
    image: easylayer:local
    cap_drop:
      - ALL
    cap_add:
      - NET_RAW
      - IPC_LOCK
    ulimits:
      memlock:
        soft: -1
        hard: -1
    devices:
      - /dev/net/tun:/dev/net/tun
    volumes:
      - /dev/hugepages:/dev/hugepages
      - ./config:/etc/easylayer:ro
    security_opt:
      - no-new-privileges:true
```

이 예시는 템플릿이다. `/dev/net/tun`, `/dev/hugepages`, `NET_RAW`, `IPC_LOCK`은 실제 기능 요구가 있을 때만 남긴다.

## 10. rootless와 userns-remap 확인

rootless 여부를 확인한다.

```bash
docker info --format '{{json .SecurityOptions}}'
docker context ls
echo "$DOCKER_HOST"
```

rootless mode에서는 보통 사용자별 Docker socket을 사용한다.

```bash
echo "$XDG_RUNTIME_DIR"
ls "$XDG_RUNTIME_DIR/docker.sock"
```

userns-remap 설정은 daemon 설정과 `/etc/subuid`, `/etc/subgid`를 확인한다.

```bash
grep -E 'dockremap|rootless|subuid' /etc/subuid /etc/subgid 2>/dev/null || true
```

구분:

| 방식 | 확인할 점 |
| --- | --- |
| non-root container user | `docker run --user`, Dockerfile `USER` |
| userns-remap | Docker daemon은 root, container UID가 host subordinate UID로 remap |
| rootless Docker | daemon과 container 모두 비root user namespace에서 실행 |

rootless는 보안 방어선을 추가하지만, packet processing workload에는 제약이 있을 수 있다. host network, privileged device, cgroup, low port binding, DPDK/VFIO 요구사항을 별도로 검토한다.

## 11. 이지레이어 hardened Compose 템플릿

control API만 필요한 기본 템플릿:

```yaml
services:
  easylayer:
    image: easylayer:local
    user: "65532:65532"
    read_only: true
    cap_drop:
      - ALL
    security_opt:
      - no-new-privileges:true
    environment:
      EASYLAYER_CONFIG: /etc/easylayer/easylayer.yaml
    volumes:
      - ./config:/etc/easylayer:ro
      - ./logs:/var/log/easylayer
    tmpfs:
      - /tmp
    networks:
      - control-net

networks:
  control-net:
    driver: bridge
```

packet capture가 필요한 경우 추가 후보:

```yaml
services:
  easylayer:
    cap_add:
      - NET_RAW
    volumes:
      - ./pcap:/var/lib/easylayer/pcap
```

interface 설정이 필요한 경우 추가 후보:

```yaml
services:
  easylayer:
    cap_add:
      - NET_ADMIN
```

DPDK/VFIO가 필요한 경우 추가 후보:

```yaml
services:
  easylayer:
    cap_add:
      - IPC_LOCK
    ulimits:
      memlock:
        soft: -1
        hard: -1
    volumes:
      - /dev/hugepages:/dev/hugepages
    devices:
      - /dev/vfio/vfio:/dev/vfio/vfio
```

운영 전 체크:

- `privileged: true`가 없는가
- Docker socket mount가 없는가
- `cap_drop: ALL`에서 필요한 capability만 추가했는가
- config mount는 read-only인가
- pcap/log/debug output은 명시적인 volume으로 빠지는가
- root filesystem이 read-only인가
- seccomp/AppArmor/SELinux를 끄지 않았는가
- non-root user로 실행 가능한가
- rootless 또는 userns-remap을 적용할 수 있는 workload인가

## 12. Kubernetes로 옮길 때 대응

Kubernetes에서는 Docker Compose 옵션을 `securityContext`로 옮긴다.

```yaml
apiVersion: v1
kind: Pod
metadata:
  name: easylayer-security-sample
spec:
  securityContext:
    runAsNonRoot: true
    seccompProfile:
      type: RuntimeDefault
  containers:
  - name: easylayer
    image: registry.example.com/easylayer:1.0.0
    securityContext:
      allowPrivilegeEscalation: false
      readOnlyRootFilesystem: true
      capabilities:
        drop: ["ALL"]
        add: ["NET_RAW"]
```

Kubernetes에서는 Pod Security Admission, RBAC, ServiceAccount, Secret mount, hostPath 정책까지 함께 봐야 한다. Docker 단일 host보다 권한 범위가 cluster로 넓어질 수 있다.

## 13. Troubleshooting

| 증상 | 확인할 내용 |
| --- | --- |
| `Operation not permitted` | capability 부족, seccomp block, AppArmor/SELinux deny를 구분한다. |
| raw socket 생성 실패 | `NET_RAW`가 있는지 확인한다. |
| route/interface 설정 실패 | `NET_ADMIN` 필요 여부와 network namespace 위치를 확인한다. |
| `Read-only file system` | `tmpfs` 또는 명시적 writable volume이 필요한지 확인한다. |
| bind mount permission denied | file ownership, SELinux `:z`/`:Z`, AppArmor deny log를 확인한다. |
| seccomp 때문에 syscall 실패 의심 | `seccomp=unconfined`으로 원인 분리 후 custom profile을 최소화한다. |
| rootless에서 network/device 기능 실패 | rootless mode 제약, cgroup, host network, device access를 확인한다. |
| Docker socket이 필요하다고 주장됨 | socket mount 없이 구현 가능한 API 구조를 먼저 검토한다. |

## 14. 정리

권한 축소 순서:

```text
non-root user
read-only root filesystem
cap_drop ALL
add only required capabilities
keep default seccomp and LSM profile
avoid Docker socket and privileged
consider userns-remap or rootless
```

이지레이어 기준:

```text
control API
  -> bridge network, non-root, no extra capability

raw capture
  -> add NET_RAW only

interface management
  -> add NET_ADMIN only if required

DPDK/VFIO
  -> explicit device, hugepage, IPC_LOCK, CPU/NUMA plan
```

## 15. 정리 명령

실습 중 남은 컨테이너를 정리한다.

```bash
docker rm -f security-lab apparmor-lab
```

이미 삭제된 컨테이너라면 오류가 날 수 있다. 생성한 항목만 지운다.

## 16. 참고 reference

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
