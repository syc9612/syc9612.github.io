# 실습 09. 직접 미니 Docker 만들기: unshare, chroot, cgroup

이 실습은 Docker가 내부에서 조합하는 Linux 커널 기능을 직접 다뤄보는 학습용 절차다. `unshare`로 namespace를 만들고, `chroot`로 root filesystem을 바꾸고, `/proc`를 mount하고, 선택적으로 cgroup과 veth/bridge 네트워크를 직접 구성한다.

이 문서는 production container runtime을 만드는 절차가 아니다. 실제 Docker는 image layer, registry, container lifecycle, logging, restart policy, network driver, volume driver, seccomp/AppArmor/SELinux, rootless mode, API server를 함께 제공한다.

## 목표

- `unshare`로 UTS, PID, mount, user, network namespace의 의미를 확인한다.
- `chroot`와 `/proc` mount가 컨테이너 rootfs 관찰에 어떤 영향을 주는지 확인한다.
- cgroup v2에서 process를 실험용 cgroup에 넣고 제한하는 흐름을 이해한다.
- veth pair와 Linux bridge로 Docker bridge network의 최소 구조를 직접 만들어 본다.
- Docker가 자동화하는 작업과 직접 구성해야 하는 작업의 차이를 구분한다.

## 전제

Linux Docker 환경 또는 disposable Linux VM을 기준으로 한다. host namespace, cgroup, network bridge를 직접 다루므로 운영 서버에서 바로 실행하지 않는다.

필요한 도구:

```bash
unshare --version
nsenter --version
ip -V
findmnt --version
docker version
```

일부 명령은 root 권한이 필요하다. network와 cgroup 실습은 host 설정에 영향을 줄 수 있으므로 절차와 정리 명령을 함께 확인한 뒤 실행한다.

Docker Desktop for Windows/macOS에서는 내부 Linux VM 경계 때문에 host network namespace, cgroup, veth/bridge 관찰이 Linux 서버와 다를 수 있다.

## 1. 실습 디렉터리와 rootfs 준비

실습 디렉터리를 만든다.

```bash
mkdir -p mini-docker-lab/rootfs
cd mini-docker-lab
```

학습용 rootfs는 Alpine image를 export해서 준비한다. 여기서는 rootfs 준비에만 Docker를 사용한다.

```bash
CID=$(docker create alpine:3.20)
docker export "$CID" | sudo tar -C rootfs -xf -
docker rm "$CID"
sudo mkdir -p rootfs/proc rootfs/sys rootfs/dev rootfs/tmp
```

확인:

```bash
ls rootfs
sudo chroot rootfs /bin/sh -c 'echo inside-rootfs; ls /'
```

주의:

- rootfs는 단순 디렉터리다.
- image layer, copy-on-write, registry, metadata는 여기서 구현하지 않는다.
- `chroot`만으로 컨테이너 보안 격리가 완성되는 것은 아니다.

## 2. UTS namespace 확인

UTS namespace는 hostname을 격리한다.

host hostname을 확인한다.

```bash
hostname
```

새 UTS namespace에서 hostname을 바꾼다.

```bash
sudo unshare --uts --fork sh -c 'hostname mini-uts; echo inside=$(hostname); sleep 1'
```

다시 host hostname을 확인한다.

```bash
hostname
```

확인할 점:

```text
새 namespace 안에서 바꾼 hostname이 host hostname을 바꾸지 않는다.
```

## 3. user namespace 확인

user namespace는 UID/GID mapping을 분리한다. 비root 사용자도 user namespace 안에서 root처럼 보일 수 있다.

```bash
unshare --user --map-root-user sh -c 'id; cat /proc/self/uid_map; cat /proc/self/gid_map'
```

예상 형태:

```text
uid=0(root) gid=0(root)
         0       1000          1
```

의미:

| 출력 | 의미 |
| --- | --- |
| namespace 내부 `uid=0` | 내부에서는 root처럼 보임 |
| `uid_map`의 host UID | host에서는 실제 사용자 UID에 매핑됨 |

user namespace는 rootless container의 핵심 기반이다. 그러나 user namespace 안에서 root처럼 보여도 host 전체 root 권한을 얻은 것은 아니다.

## 4. PID namespace와 `/proc` mount

PID namespace를 만들 때는 `--fork`가 중요하다. 새 PID namespace에서 fork된 child process가 PID 1이 된다.

```bash
sudo unshare --fork --pid --mount-proc sh -c 'echo "pid=$$"; ps'
```

예상:

```text
pid=1
PID   USER     TIME  COMMAND
1     root     ...   sh
...
```

`--mount-proc` 없이 PID namespace만 바꾸면 `/proc`가 host 기준으로 남아 출력이 혼동될 수 있다. PID namespace 관찰에는 PID namespace와 mount namespace, `/proc` mount를 함께 봐야 한다.

## 5. chroot와 PID namespace 조합

rootfs 안에서 새 PID namespace를 실행한다.

```bash
ROOTFS=$(readlink -f rootfs)
sudo env ROOTFS="$ROOTFS" unshare --fork --pid --mount sh -c '
  mount --make-rprivate /
  mount -t proc proc "$ROOTFS/proc"
  chroot "$ROOTFS" /bin/sh -c "echo pid=\$\$; hostname; ps; mount | grep proc"
  umount "$ROOTFS/proc"
'
```

확인할 점:

| 항목 | 의미 |
| --- | --- |
| `pid=1` | chroot 안에서 실행한 shell이 새 PID namespace의 PID 1 |
| `/` 내용 | host root가 아니라 `rootfs` 디렉터리 |
| `/proc` | 새 PID namespace 기준으로 mount된 procfs |

주의:

- `chroot`는 root directory를 바꾸는 기능이지 완전한 container security boundary가 아니다.
- mount namespace를 함께 쓰지 않으면 mount/unmount가 host에 영향을 줄 수 있다.
- `mount --make-rprivate /`는 실험 중 mount propagation을 host와 분리하기 위한 방어적 설정이다.

## 6. mount namespace 확인

mount namespace 안에서 tmpfs를 mount하고 host에는 보이지 않는지 확인한다.

```bash
sudo mkdir -p /tmp/mini-mnt-host
sudo unshare --mount --fork sh -c '
  mount --make-rprivate /
  mount -t tmpfs tmpfs /tmp/mini-mnt-host
  echo "inside" > /tmp/mini-mnt-host/file
  findmnt /tmp/mini-mnt-host
  sleep 1
'
findmnt /tmp/mini-mnt-host || true
sudo rmdir /tmp/mini-mnt-host
```

확인할 점:

```text
unshare 내부에서는 tmpfs mount가 보이지만, 명령이 끝난 뒤 host에서는 mount가 남지 않는다.
```

## 7. cgroup v2 선택 실습

cgroup은 process가 사용할 수 있는 resource를 제한하고 계측한다. 이 단계는 host cgroup tree에 실험용 디렉터리를 만들기 때문에 disposable VM에서만 실행한다.

cgroup v2 mount 여부를 확인한다.

```bash
findmnt -t cgroup2
cat /sys/fs/cgroup/cgroup.controllers
```

실험용 cgroup을 만든다.

```bash
CG=/sys/fs/cgroup/mini-docker-lab
sudo mkdir -p "$CG"
```

`pids.max`를 설정하고 child shell을 cgroup에 넣는다.

```bash
sudo sh -c '
  CG=/sys/fs/cgroup/mini-docker-lab
  echo 20 > "$CG/pids.max"
  echo $$ > "$CG/cgroup.procs"
  echo "current cgroup:"
  cat /proc/self/cgroup
  echo "pids.max=$(cat "$CG/pids.max")"
'
```

메모리 제한 예시:

```bash
sudo sh -c '
  CG=/sys/fs/cgroup/mini-docker-lab
  echo 128M > "$CG/memory.max"
  cat "$CG/memory.max"
'
```

정리:

```bash
sudo rmdir /sys/fs/cgroup/mini-docker-lab
```

주의:

- systemd가 관리하는 host에서는 controller 활성화와 delegation 정책 때문에 일부 파일 쓰기가 실패할 수 있다.
- root cgroup의 `cgroup.subtree_control`을 함부로 바꾸지 않는다.
- cgroup 실습은 namespace 실습과 달리 host resource policy에 직접 영향을 줄 수 있다.

## 8. network namespace와 veth/bridge 선택 실습

이 단계는 host network에 bridge와 veth를 만든다. 이미 같은 이름의 interface가 있는지 먼저 확인한다.

```bash
ip link show mini-br0 || true
ip netns list | grep mini-net || true
```

network namespace와 veth pair를 만든다.

```bash
sudo ip netns add mini-net
sudo ip link add veth-host type veth peer name veth-mini
sudo ip link set veth-mini netns mini-net
```

host bridge를 만든다.

```bash
sudo ip link add mini-br0 type bridge
sudo ip addr add 10.200.0.1/24 dev mini-br0
sudo ip link set mini-br0 up
sudo ip link set veth-host master mini-br0
sudo ip link set veth-host up
```

container 쪽 namespace를 설정한다.

```bash
sudo ip netns exec mini-net ip addr add 10.200.0.2/24 dev veth-mini
sudo ip netns exec mini-net ip link set lo up
sudo ip netns exec mini-net ip link set veth-mini up
sudo ip netns exec mini-net ip route add default via 10.200.0.1
```

ping으로 host bridge와 통신한다.

```bash
sudo ip netns exec mini-net ping -c 1 10.200.0.1
```

구조:

```text
mini-net namespace
  veth-mini 10.200.0.2/24
    |
host namespace
  veth-host
    |
  mini-br0 10.200.0.1/24
```

외부 통신까지 필요하면 NAT와 forwarding을 추가해야 한다. 이 작업은 host firewall에 영향을 주므로 이 문서에서는 기본 실습 범위에서 제외한다.

## 9. rootfs를 network namespace에서 실행

준비한 rootfs를 `mini-net` network namespace 안에서 실행한다.

```bash
ROOTFS=$(readlink -f rootfs)
sudo env ROOTFS="$ROOTFS" ip netns exec mini-net unshare --fork --pid --mount sh -c '
  mount --make-rprivate /
  mount -t proc proc "$ROOTFS/proc"
  chroot "$ROOTFS" /bin/sh -c "echo pid=\$\$; ip addr; ip route; ping -c 1 10.200.0.1"
  umount "$ROOTFS/proc"
'
```

확인할 점:

| 항목 | 의미 |
| --- | --- |
| PID | chroot 안의 process가 새 PID namespace에서 PID 1로 보임 |
| IP | `veth-mini`에 설정한 `10.200.0.2/24`가 보임 |
| route | default gateway가 `10.200.0.1`로 보임 |
| ping | host bridge와 통신 가능 |

이 단계가 Docker 기본 bridge network의 최소 형태에 가깝다. Docker는 여기에 IPAM, DNS, NAT, firewall, lifecycle, logging, cleanup을 자동화한다.

## 10. pivot_root 개념

`pivot_root`는 mount namespace 안에서 현재 root를 새 root로 바꾸고 old root를 새 root 안의 디렉터리로 이동한다. 실제 runtime은 `chroot`보다 `pivot_root` 또는 유사한 rootfs 전환을 사용한다.

학습 단계에서는 다음 차이만 기억한다.

| 방식 | 특징 |
| --- | --- |
| `chroot` | 프로세스가 보는 root directory를 바꿈. 보안 경계로는 부족함. |
| `pivot_root` | mount namespace 안에서 root mount 자체를 교체하고 old root를 분리 가능. |

`pivot_root`는 mount 조건이 까다롭고 실수 시 shell이 혼동될 수 있으므로, 이 실습에서는 명령 실행보다 개념 정리에 둔다. 직접 실험하려면 disposable VM에서 별도 rootfs와 mount namespace를 사용한다.

## 11. Docker가 추가로 해주는 일

위 실습으로 확인한 것은 container runtime의 아주 작은 부분이다.

Docker가 추가로 처리하는 일:

- image pull, unpack, layer merge
- writable layer 생성
- OCI runtime spec 생성
- namespace, cgroup, capability, seccomp, AppArmor/SELinux 설정
- network driver, IPAM, DNS, NAT, port publishing
- volume mount와 mount propagation 처리
- log driver, restart policy, healthcheck
- container lifecycle API와 event 관리
- cleanup과 orphan resource 회수

따라서 “미니 Docker”는 Docker 대체품이 아니라 Docker의 내부 구성 요소를 이해하기 위한 실험이다.

## 12. Troubleshooting

| 증상 | 확인할 내용 |
| --- | --- |
| `unshare: Operation not permitted` | user namespace 허용 여부, root 권한, capability 정책 확인 |
| `ps`가 host process를 보여줌 | PID namespace만 만들고 `/proc`를 새로 mount하지 않았을 수 있음 |
| `chroot: failed to run command` | rootfs에 `/bin/sh`와 필요한 library가 있는지 확인 |
| `/proc` umount 실패 | chroot 내부 process가 남아 있거나 cwd가 mount 안에 있는지 확인 |
| cgroup 파일 쓰기 실패 | cgroup v2 여부, systemd delegation, controller 활성화 상태 확인 |
| `ip netns add` 실패 | root 권한과 `iproute2` 설치 여부 확인 |
| namespace에서 ping 실패 | veth link up, bridge IP, route, firewall 확인 |
| bridge 삭제 실패 | veth 또는 namespace가 남아 있는지 확인 |

## 13. 정리 명령

network 실습을 했다면 먼저 network 리소스를 지운다.

```bash
sudo ip netns del mini-net
sudo ip link del mini-br0
```

남아 있는 veth가 있는지 확인한다.

```bash
ip link show | grep veth || true
```

cgroup 실습을 했다면 남은 cgroup을 지운다.

```bash
sudo rmdir /sys/fs/cgroup/mini-docker-lab 2>/dev/null || true
```

실습 디렉터리를 지운다.

```bash
cd ..
sudo rm -rf mini-docker-lab
```

주의:

- `rm -rf`는 실습 디렉터리의 상위 디렉터리에서 경로를 확인한 뒤 실행한다.
- network namespace 또는 bridge 이름을 바꿔 사용했다면 해당 이름으로 정리한다.

## 14. 참고 reference

- [Linux namespaces](https://man7.org/linux/man-pages/man7/namespaces.7.html)
- [unshare(1)](https://man7.org/linux/man-pages/man1/unshare.1.html)
- [chroot(2)](https://man7.org/linux/man-pages/man2/chroot.2.html)
- [pivot_root(2)](https://man7.org/linux/man-pages/man2/pivot_root.2.html)
- [mount namespaces](https://man7.org/linux/man-pages/man7/mount_namespaces.7.html)
- [PID namespaces](https://man7.org/linux/man-pages/man7/pid_namespaces.7.html)
- [network namespaces](https://man7.org/linux/man-pages/man7/network_namespaces.7.html)
- [veth(4)](https://man7.org/linux/man-pages/man4/veth.4.html)
- [cgroups(7)](https://man7.org/linux/man-pages/man7/cgroups.7.html)
- [Control Group v2 kernel documentation](https://www.kernel.org/doc/html/latest/admin-guide/cgroup-v2.html)
