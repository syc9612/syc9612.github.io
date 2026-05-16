# 실습 07. Packet processing 시스템에서 Docker 쓰는 방식

이 실습은 패킷 처리 애플리케이션을 Docker로 실행할 때 bridge, host, macvlan, ipvlan, raw socket, AF_XDP, DPDK 계열을 어떤 기준으로 구분해야 하는지 정리한다.

실제 이지레이어가 어떤 packet I/O 방식을 쓰는지 확정되기 전까지는 모든 옵션을 적용하지 않는다. 먼저 host와 컨테이너에서 관찰 가능한 네트워크 경로, 권한, CPU/NUMA/IRQ 조건을 확인하고, 기능 요구사항에서 필요한 옵션만 선택한다.

## 목표

- bridge, host, macvlan, ipvlan의 packet path와 trade-off를 비교한다.
- raw socket, libpcap, AF_XDP, DPDK가 Docker 설계에 미치는 차이를 구분한다.
- capability, device mount, hugepage, CPU pinning, NUMA, IRQ affinity 확인 항목을 정리한다.
- 이지레이어를 Compose 또는 Kubernetes로 올릴 때 control API와 packet path를 분리하는 기준을 잡는다.

## 전제

Linux Docker 환경을 기준으로 한다.

Docker Desktop for Windows/macOS에서는 Linux 컨테이너가 내부 VM에서 실행되므로 host NIC, IRQ, NUMA, `/dev/vfio`, `/dev/hugepages`, AF_XDP 같은 항목을 host OS에서 그대로 관찰하기 어렵다. packet processing 실험은 실제 Linux 서버, Linux VM, 또는 WSL2 내부 Linux에서 진행하는 편이 낫다.

일부 명령은 root 권한이 필요하다. 이 문서의 macvlan/ipvlan/DPDK 관련 명령은 환경에 맞는 NIC 이름, subnet, gateway, PCI 주소를 확인한 뒤 실행해야 한다.

## 1. 먼저 packet I/O 방식 분류

애플리케이션이 어떤 방식으로 packet을 다루는지 먼저 분류한다.

| 방식 | 확인 질문 | Docker 영향 |
| --- | --- | --- |
| 일반 TCP/UDP socket | `listen`, `connect`, HTTP/gRPC 같은 일반 socket인가 | bridge 또는 host network 선택 |
| raw socket / AF_PACKET / libpcap | packet capture, L2 frame, promiscuous mode가 필요한가 | `NET_RAW`, interface visibility, host network 검토 |
| TUN/TAP | `/dev/net/tun`을 여는가 | device mount와 `NET_ADMIN` 필요 |
| AF_XDP | XDP program, UMEM, queue id를 쓰는가 | kernel/NIC/driver 지원, BPF 권한, queue 설계 필요 |
| DPDK | EAL, hugepage, VFIO/UIO, PCI BDF를 쓰는가 | host hugepage, device binding, CPU/NUMA 배치 필요 |

이지레이어에서 먼저 확인해야 할 항목:

- packet I/O library 이름: libpcap, AF_PACKET, AF_XDP, DPDK, 일반 socket 중 무엇인가
- NIC를 직접 잡는지, pcap file만 읽는지
- interface 설정이나 route/qdisc 조작이 필요한지
- control API와 packet path가 같은 프로세스인지 분리 가능한지
- 성능 실험이 필요한 core, queue, NUMA 조건이 있는지

## 2. host 네트워크 기본 정보 확인

host NIC, route, CPU, NUMA 정보를 확인한다.

```bash
ip -br addr
ip route
lscpu -e
numactl -H
```

`numactl`이 없다면 설치하거나 다음으로 대체한다.

```bash
lscpu | grep -E 'NUMA|CPU\\(s\\)'
```

PowerShell에서 WSL2 또는 Linux SSH가 아니라 Windows host에서 실행 중이라면 이 정보는 Docker Desktop 내부 VM과 다를 수 있다. Docker 관련 network namespace 실험은 Linux shell에서 진행한다.

NIC interrupt 상태를 본다.

```bash
cat /proc/interrupts | grep -E 'CPU|eth|ens|enp|eno'
```

IRQ affinity를 확인하려면 특정 IRQ 번호를 잡아 본다.

```bash
IRQ=<irq-number>
cat /proc/irq/"$IRQ"/smp_affinity_list
```

이 값을 직접 바꾸는 작업은 host 전체 성능에 영향을 줄 수 있으므로 실습에서는 확인만 한다.

## 3. bridge network 경로 확인

기본 bridge network에 컨테이너를 띄운다.

```bash
docker run -d --name packet-bridge-lab alpine sleep 1d
docker inspect -f '{{.NetworkSettings.Networks.bridge.IPAddress}}' packet-bridge-lab
docker exec packet-bridge-lab ip addr
docker exec packet-bridge-lab ip route
```

host에서 bridge와 veth를 본다.

```bash
ip link show docker0
bridge link
```

Docker firewall/NAT 규칙을 확인한다.

```bash
sudo iptables -t nat -S | grep DOCKER
sudo iptables -S | grep DOCKER
```

nftables backend 환경이면 다음도 확인한다.

```bash
sudo nft list ruleset | grep -i docker
```

확인할 점:

| 항목 | 의미 |
| --- | --- |
| container `eth0` | container namespace 안의 veth |
| host veth | `docker0`에 붙은 peer |
| `docker0` | L2 bridge 역할 |
| NAT rule | 외부 송신과 port publishing에 관여 |

bridge network는 관리가 쉽지만 packet path에 veth, bridge, netfilter, conntrack이 섞인다. 성능 측정 목적이면 이 경로가 결과에 포함된다는 점을 기록한다.

## 4. host network 확인

host network는 container network namespace를 따로 만들지 않고 host network stack을 공유한다.

```bash
docker run --rm --network host alpine ip addr
docker run --rm --network host alpine ip route
```

특징:

| 항목 | bridge | host |
| --- | --- | --- |
| network namespace | 컨테이너별 분리 | host와 공유 |
| port publishing | `ports`/DNAT 사용 | 보통 사용하지 않음 |
| interface visibility | 컨테이너 인터페이스 중심 | host NIC가 그대로 보임 |
| 격리 | 상대적으로 강함 | 약함 |
| 성능 실험 | Docker 경로가 섞임 | host stack 기준에 가까움 |

Compose에서는 다음처럼 쓴다.

```yaml
services:
  easylayer:
    image: easylayer:local
    network_mode: host
```

주의:

- host network에서는 Compose `ports`를 함께 쓰지 않는다.
- 같은 host port를 여러 컨테이너가 동시에 사용할 수 없다.
- Docker Desktop에서는 Linux Docker와 동작 차이가 있을 수 있다.

## 5. macvlan과 ipvlan 설계 확인

macvlan과 ipvlan은 컨테이너를 물리망에 더 직접적으로 붙이는 방식이다. 실제 subnet과 parent interface를 모르면 실행하지 않는다.

host의 parent interface를 확인한다.

```bash
ip -br link
ip -br addr
```

macvlan 예시:

```bash
docker network create -d macvlan \
  --subnet=192.0.2.0/24 \
  --gateway=192.0.2.1 \
  -o parent=eth0 packet-macvlan
```

ipvlan L2 예시:

```bash
docker network create -d ipvlan \
  --subnet=192.0.2.0/24 \
  --gateway=192.0.2.1 \
  -o ipvlan_mode=l2 \
  -o parent=eth0 packet-ipvlan
```

테스트 컨테이너:

```bash
docker run --rm --network packet-macvlan alpine ip addr
docker run --rm --network packet-ipvlan alpine ip addr
```

설계 기준:

| 방식 | 장점 | 주의점 |
| --- | --- | --- |
| macvlan | 컨테이너가 별도 MAC을 가진 장비처럼 보임 | switch MAC table, promisc, host와 직접 통신 제약 |
| ipvlan L2 | parent MAC을 공유해 MAC 수를 줄임 | L2 gateway와 host 통신 모델 이해 필요 |
| ipvlan L3 | routing 중심으로 분리 가능 | route 설계가 명확해야 함 |

Docker 공식 문서 기준으로 macvlan 컨테이너는 host와 직접 통신이 제한될 수 있다. host와 통신이 필요하면 별도 bridge network를 추가하거나 host 쪽 macvlan interface를 별도로 설계한다.

정리:

```bash
docker network rm packet-macvlan packet-ipvlan
```

위 정리 명령은 실제로 네트워크를 만든 경우에만 실행한다.

## 6. raw socket 권한 확인

raw socket 또는 AF_PACKET을 쓰는 프로그램은 보통 `NET_RAW`가 필요하다. 다음 예시는 Python으로 AF_PACKET raw socket 생성만 확인한다.

권한 없이 실행:

```bash
docker run --rm python:3.12-alpine python -c "import socket; socket.socket(socket.AF_PACKET, socket.SOCK_RAW, socket.htons(3)); print('ok')"
```

예상:

```text
PermissionError 또는 Operation not permitted
```

`NET_RAW`를 추가해서 실행:

```bash
docker run --rm --cap-drop ALL --cap-add NET_RAW python:3.12-alpine python -c "import socket; socket.socket(socket.AF_PACKET, socket.SOCK_RAW, socket.htons(3)); print('ok')"
```

예상:

```text
ok
```

Compose 예시:

```yaml
services:
  packet-capture:
    image: easylayer:local
    cap_drop:
      - ALL
    cap_add:
      - NET_RAW
    security_opt:
      - no-new-privileges:true
```

host NIC를 직접 보고 capture해야 한다면 `network_mode: host`도 검토한다. bridge network에서는 컨테이너 namespace 안의 interface만 보이므로 관찰 위치가 달라진다.

## 7. TUN/TAP 또는 interface 설정 권한

TUN/TAP, route, qdisc, interface up/down 같은 조작은 `NET_ADMIN`이 필요할 수 있다.

Compose 예시:

```yaml
services:
  tunnel-worker:
    image: easylayer:local
    cap_drop:
      - ALL
    cap_add:
      - NET_ADMIN
    devices:
      - /dev/net/tun:/dev/net/tun
```

확인할 점:

- 애플리케이션이 실제로 `/dev/net/tun`을 여는가
- host route나 qdisc를 바꾸는가, container namespace 안에서만 바꾸는가
- `NET_ADMIN`이 필요한 명령 범위를 문서화했는가
- 운영에서는 `privileged: true` 대신 필요한 device와 capability만 열었는가

## 8. AF_XDP 점검 항목

AF_XDP는 컨테이너 옵션보다 host kernel, NIC driver, XDP 지원 여부가 먼저다.

host에서 확인한다.

```bash
uname -r
ethtool -i eth0
ethtool -l eth0
```

XDP 기능 확인은 배포판과 kernel/tool 버전에 따라 다르다. 가능한 환경에서는 다음을 확인한다.

```bash
ip link show dev eth0
bpftool feature probe
```

컨테이너 설계에서 확인할 항목:

| 항목 | 이유 |
| --- | --- |
| hostNetwork 여부 | host NIC와 queue를 직접 기준으로 잡는 경우가 많음 |
| BPF filesystem | XDP program pinning이나 관찰에 `/sys/fs/bpf`가 필요할 수 있음 |
| capability | XDP attach와 BPF load에 `NET_ADMIN`, `BPF`, `PERFMON` 등이 필요할 수 있음 |
| queue id | AF_XDP socket은 netdev와 queue id에 묶임 |
| copy/zero-copy | driver와 NIC가 zero-copy를 지원하는지 확인 |

템플릿:

```yaml
services:
  af-xdp-worker:
    image: easylayer:local
    network_mode: host
    cap_drop:
      - ALL
    cap_add:
      - NET_ADMIN
      - NET_RAW
      - BPF
      - PERFMON
    volumes:
      - /sys/fs/bpf:/sys/fs/bpf
      - ./config:/etc/easylayer:ro
```

주의:

- `BPF`, `PERFMON` capability 지원은 host kernel과 container runtime에 따라 다를 수 있다.
- 개발 중에는 권한을 넓게 잡고 원인을 찾더라도, 운영 manifest는 실제 필요한 capability로 다시 줄인다.
- AF_XDP는 NIC driver별 차이가 크므로 공식 driver 문서와 kernel feature를 함께 확인한다.

## 9. DPDK 점검 항목

DPDK 계열은 host 준비가 먼저다.

host에서 hugepage 상태를 확인한다.

```bash
grep -i huge /proc/meminfo
findmnt | grep -i huge
ls -ld /dev/hugepages
```

PCI NIC와 driver를 확인한다.

```bash
lspci -nn | grep -i ethernet
```

특정 PCI device의 driver를 확인한다.

```bash
PCI=0000:03:00.0
readlink /sys/bus/pci/devices/"$PCI"/driver
```

DPDK에서 확인할 host 조건:

| 항목 | 확인 이유 |
| --- | --- |
| hugepage | DPDK memory pool과 DMA buffer에 필요 |
| VFIO/UIO driver | NIC를 kernel network stack 대신 user-space driver로 다루기 위함 |
| IOMMU | VFIO 사용 시 중요 |
| PCI BDF | 애플리케이션 EAL `-a` allow list와 연결 |
| NUMA node | NIC와 가까운 CPU/memory를 쓰기 위함 |
| CPU core | DPDK `--lcores`, Docker `cpuset`과 일치해야 함 |

Compose 템플릿:

```yaml
services:
  dpdk-worker:
    image: easylayer:local
    network_mode: host
    cap_drop:
      - ALL
    cap_add:
      - IPC_LOCK
    ulimits:
      memlock:
        soft: -1
        hard: -1
    cpuset: "2-5"
    volumes:
      - /dev/hugepages:/dev/hugepages
      - ./config:/etc/easylayer:ro
    devices:
      - /dev/vfio/vfio:/dev/vfio/vfio
      - /dev/vfio/42:/dev/vfio/42
    command:
      - easylayer
      - --config
      - /etc/easylayer/easylayer.yaml
      - --
      - --lcores=2-5
      - -a
      - "0000:03:00.0"
```

주의:

- `/dev/vfio/42`는 예시다. 실제 IOMMU group 번호를 확인해야 한다.
- DPDK가 VFIO가 아니라 AF_PACKET, pcap PMD, tap PMD를 쓰는 경우 장치 요구사항이 달라진다.
- NIC를 DPDK driver에 bind하면 host kernel network interface로는 보이지 않을 수 있다.
- host 운영 NIC를 잘못 bind하면 SSH 연결이 끊길 수 있다.

## 10. CPU, NUMA, IRQ 배치 확인

CPU topology:

```bash
lscpu -e=CPU,CORE,SOCKET,NODE,ONLINE
```

NIC NUMA node:

```bash
PCI=0000:03:00.0
cat /sys/bus/pci/devices/"$PCI"/numa_node
```

NIC queue:

```bash
ethtool -l eth0
ethtool -x eth0
```

IRQ 분포:

```bash
cat /proc/interrupts | grep -E 'CPU|eth0|ens|enp'
```

설계 기준:

| 항목 | 기준 |
| --- | --- |
| Docker `cpuset` | 컨테이너가 실행 가능한 CPU 범위 |
| DPDK `--lcores` | 애플리케이션이 사용할 logical core mapping |
| NIC NUMA node | worker core와 memory allocation을 가까운 node에 배치 |
| IRQ affinity | kernel stack 기반 packet path에서는 NIC interrupt 위치가 중요 |
| housekeeping CPU | isolated core를 쓰는 경우 OS noise를 다른 CPU로 이동 |

Docker `cpuset`과 DPDK `--lcores`가 충돌하지 않게 한다. 예를 들어 Compose에서 `cpuset: "2-5"`를 줬다면 애플리케이션도 그 범위 안의 core만 쓰도록 맞춘다.

## 11. 이지레이어 Compose 설계안

control API와 packet path를 나누는 출발점:

```yaml
services:
  easylayer-control:
    image: easylayer:local
    command: ["easylayer-control", "--config", "/etc/easylayer/control.yaml"]
    networks:
      - control-net
    ports:
      - "8080:8080"
    volumes:
      - ./config:/etc/easylayer:ro

  easylayer-packet:
    image: easylayer:local
    command: ["easylayer-packet", "--config", "/etc/easylayer/packet.yaml"]
    network_mode: host
    cap_drop:
      - ALL
    cap_add:
      - NET_RAW
      - NET_ADMIN
    volumes:
      - ./config:/etc/easylayer:ro
      - ./pcap:/var/lib/easylayer/pcap

networks:
  control-net:
    driver: bridge
```

이 구조가 맞는지 확인할 질문:

- control process와 packet process를 실제로 분리할 수 있는가
- packet process가 일반 socket인지 raw socket인지 AF_XDP/DPDK인지
- `NET_ADMIN`이 정말 필요한지, `NET_RAW`만으로 충분한지
- packet output이 writable layer가 아니라 volume으로 빠지는지
- host network port 충돌 가능성이 있는지

## 12. Kubernetes로 연결할 때

Kubernetes에서는 packet path가 node-local이면 DaemonSet을 먼저 검토한다.

```yaml
apiVersion: apps/v1
kind: DaemonSet
metadata:
  name: easylayer-packet
spec:
  selector:
    matchLabels:
      app: easylayer-packet
  template:
    metadata:
      labels:
        app: easylayer-packet
    spec:
      hostNetwork: true
      dnsPolicy: ClusterFirstWithHostNet
      containers:
      - name: packet
        image: registry.example.com/easylayer:1.0.0
        securityContext:
          allowPrivilegeEscalation: false
          capabilities:
            drop: ["ALL"]
            add: ["NET_RAW", "NET_ADMIN"]
        resources:
          requests:
            cpu: "2"
            memory: "512Mi"
```

DPDK나 SR-IOV가 필요하면 device plugin, hugepage resource, CPU Manager, Topology Manager까지 이어서 설계해야 한다. 이 부분은 단순 manifest 변환이 아니라 cluster node 운영 정책과 연결된다.

## 13. Troubleshooting

| 증상 | 확인할 내용 |
| --- | --- |
| bridge에서는 되지만 host network에서 port 충돌 | host에 같은 port를 쓰는 프로세스가 있는지 확인 |
| raw socket 생성 실패 | `NET_RAW` capability가 있는지 확인 |
| interface 설정 실패 | `NET_ADMIN` 필요 여부와 namespace 위치 확인 |
| macvlan 컨테이너가 host와 통신 안 됨 | macvlan의 host 직접 통신 제약과 별도 host macvlan interface 필요 여부 확인 |
| AF_XDP attach 실패 | kernel, NIC driver, queue id, XDP mode, BPF capability 확인 |
| DPDK EAL 초기화 실패 | hugepage, `--lcores`, PCI allow list, VFIO/UIO binding, IOMMU group 확인 |
| 성능이 불안정함 | CPU pinning, IRQ affinity, NUMA locality, 로그 출력, power management 확인 |
| Docker Desktop에서 NIC가 다르게 보임 | 내부 Linux VM 경계를 고려하고 Linux 환경에서 재검증 |

## 14. 정리

선택 흐름:

```text
control API only
  -> bridge network

host network stack visibility needed
  -> host network

L2/L3 endpoint처럼 보여야 함
  -> macvlan or ipvlan

raw packet capture needed
  -> NET_RAW, host network 검토

XDP queue 기반 고성능 path
  -> AF_XDP, kernel/NIC/driver/BPF 권한 확인

user-space NIC ownership
  -> DPDK, hugepage/VFIO/CPU/NUMA 설계
```

권한 원칙:

```text
start from required packet I/O feature
derive exact capability and device
avoid privileged as a default
record host prerequisites outside Compose
```

## 15. 정리 명령

실습에서 생성한 기본 컨테이너를 정리한다.

```bash
docker rm -f packet-bridge-lab
```

macvlan/ipvlan 네트워크를 실제로 만들었다면 정리한다.

```bash
docker network rm packet-macvlan packet-ipvlan
```

없는 리소스라면 오류가 날 수 있다. 생성한 항목만 지운다.

## 16. 참고 reference

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
