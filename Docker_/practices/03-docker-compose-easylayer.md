# 실습 03. Docker Compose 실전: 이지레이어 빌드 고려

이 실습은 이지레이어 같은 C++ 기반 네트워크/패킷 처리 서비스를 Docker Compose로 빌드하고 실행할 때 고려해야 할 구조를 정리한다.

실제 이지레이어의 디렉터리와 빌드 명령은 프로젝트에 맞게 바꿔야 한다. 여기서는 Compose 설계 기준과 예시 템플릿을 중심으로 다룬다.

## 목표

- Compose로 빌드, 실행, 설정 mount를 분리한다.
- 개발용 Compose와 성능/네트워크 검증용 Compose의 차이를 이해한다.
- 이지레이어 빌드 시 필요한 capability, network mode, volume, device 옵션을 검토한다.
- `privileged: true` 없이 필요한 권한만 추가하는 방향을 잡는다.

## 1. 권장 파일 구조

예시:

```text
project-root/
  Dockerfile
  compose.yaml
  compose.override.yaml
  config/
    easylayer.yaml
  logs/
  src/
  CMakeLists.txt
```

역할:

| 파일/디렉터리 | 역할 |
| --- | --- |
| `Dockerfile` | 이지레이어 이미지 빌드 |
| `compose.yaml` | 공통 실행 정의 |
| `compose.override.yaml` | 로컬 개발용 override |
| `config/` | 런타임 설정 파일 |
| `logs/` | 컨테이너 로그 또는 디버그 출력 |
| `src/` | 애플리케이션 소스 |

## 2. multi-stage Dockerfile 예시

C++ 프로젝트는 빌드 도구와 런타임 의존성을 분리하는 편이 좋다.

```dockerfile
FROM ubuntu:24.04 AS build

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    cmake \
    ninja-build \
    pkg-config \
    ca-certificates \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY . .

RUN cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release \
  && cmake --build build

FROM ubuntu:24.04 AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libstdc++6 \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=build /src/build/easylayer /usr/local/bin/easylayer

ENTRYPOINT ["easylayer"]
```

주의할 점:

- 빌드 이미지에는 컴파일러와 CMake를 둔다.
- 런타임 이미지에는 실행에 필요한 라이브러리만 둔다.
- 패킷 처리 라이브러리, DPDK, libpcap, eBPF 관련 의존성은 실제 프로젝트에 맞게 추가한다.
- 빌드 결과 바이너리 경로는 실제 산출물 이름에 맞게 수정한다.

## 3. 기본 compose.yaml 예시

일반 bridge network에서 제어 API 또는 테스트용 UDP/TCP 서비스를 실행하는 형태다.

```yaml
services:
  easylayer:
    build:
      context: .
      dockerfile: Dockerfile
      target: runtime
    image: easylayer:local
    container_name: easylayer
    init: true
    environment:
      EASYLAYER_CONFIG: /etc/easylayer/easylayer.yaml
      EASYLAYER_LOG_LEVEL: info
    volumes:
      - ./config:/etc/easylayer:ro
      - ./logs:/var/log/easylayer
    ports:
      - "8080:8080"
    networks:
      - easylayer-net
    restart: unless-stopped

networks:
  easylayer-net:
    driver: bridge
```

확인:

```bash
docker compose build
docker compose up -d
docker compose ps
docker compose logs -f easylayer
```

## 4. 개발용 override 예시

개발 중에는 소스 bind mount와 디버그 빌드가 필요할 수 있다.

```yaml
services:
  easylayer:
    build:
      target: build
    working_dir: /src
    volumes:
      - .:/src
      - ./config:/etc/easylayer:ro
      - ./logs:/var/log/easylayer
    command: >
      sh -lc "cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Debug
      && cmake --build build
      && ./build/easylayer --config /etc/easylayer/easylayer.yaml"
```

실행:

```bash
docker compose up --build
```

주의:

- Windows/macOS bind mount는 Linux native보다 파일 I/O가 느릴 수 있다.
- C++ 빌드 산출물은 별도 named volume에 둘 수도 있다.
- 개발 편의와 운영 재현성을 같은 Compose 파일에 억지로 섞지 않는 편이 좋다.

## 5. host network 모드 예시

패킷 경로를 host와 최대한 동일하게 보고 싶다면 Linux에서 host network를 쓸 수 있다.

```yaml
services:
  easylayer:
    image: easylayer:local
    network_mode: host
    init: true
    volumes:
      - ./config:/etc/easylayer:ro
      - ./logs:/var/log/easylayer
    environment:
      EASYLAYER_CONFIG: /etc/easylayer/easylayer.yaml
```

특징:

| 항목 | 설명 |
| --- | --- |
| `ports` | host network에서는 보통 사용하지 않는다. |
| 포트 충돌 | 컨테이너가 host 포트를 직접 사용하므로 충돌 가능성이 있다. |
| 격리 | bridge보다 네트워크 격리가 약하다. |
| Docker Desktop | Windows/macOS에서는 Linux와 동작 차이가 있을 수 있다. |

## 6. capability 최소 부여

패킷 캡처, raw socket, 인터페이스 설정이 필요하면 capability가 필요할 수 있다.

```yaml
services:
  easylayer:
    image: easylayer:local
    cap_drop:
      - ALL
    cap_add:
      - NET_RAW
      - NET_ADMIN
    security_opt:
      - no-new-privileges:true
```

의미:

| capability | 필요한 경우 |
| --- | --- |
| `NET_RAW` | raw socket, ping, 일부 packet capture |
| `NET_ADMIN` | interface 설정, routing, qdisc, iptables 조작 |
| `IPC_LOCK` | hugepage, memory lock이 필요한 packet processing |

`privileged: true`는 모든 장치를 넓게 열어주므로 디버깅이 아닌 경우 피하는 것이 좋다. 필요한 capability와 device만 명시하는 방향이 낫다.

## 7. device와 hugepage 예시

DPDK 또는 AF_XDP 계열이라면 장치와 hugepage가 필요할 수 있다.

```yaml
services:
  easylayer:
    image: easylayer:local
    cap_add:
      - NET_ADMIN
      - NET_RAW
      - IPC_LOCK
    ulimits:
      memlock:
        soft: -1
        hard: -1
    volumes:
      - /dev/hugepages:/dev/hugepages
      - ./config:/etc/easylayer:ro
    devices:
      - /dev/net/tun:/dev/net/tun
```

주의:

- 실제 NIC device를 컨테이너에 넘기는 방식은 환경마다 다르다.
- DPDK는 NIC binding, IOMMU, hugepage 설정이 host에서 먼저 준비되어야 한다.
- AF_XDP는 kernel, driver, NIC 지원 여부를 확인해야 한다.

## 8. CPU와 성능 고려

패킷 처리 서비스는 CPU 배치가 중요할 수 있다.

```yaml
services:
  easylayer:
    image: easylayer:local
    cpuset: "2-3"
```

확인할 내용:

| 항목 | 이유 |
| --- | --- |
| `cpuset` | 특정 CPU core에 고정 |
| NUMA | NIC와 가까운 CPU/memory 사용 |
| interrupt affinity | NIC interrupt와 worker thread 배치 |
| 로그 수준 | 과도한 로그는 packet path 성능을 떨어뜨릴 수 있음 |

Compose는 간단한 CPU 제한은 줄 수 있지만, 고급 NUMA/IRQ 튜닝은 host 설정과 함께 봐야 한다.

## 9. healthcheck 예시

이지레이어가 control API를 제공한다면 healthcheck를 넣는 것이 좋다.

```yaml
services:
  easylayer:
    image: easylayer:local
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8080/healthz"]
      interval: 10s
      timeout: 3s
      retries: 3
      start_period: 10s
```

control API가 없다면 다음 중 하나를 고려한다.

- 프로세스 상태 확인용 별도 command 제공
- UNIX socket health endpoint 제공
- metrics endpoint 제공
- 로그 기반 readiness 판단은 되도록 피하기

## 10. 운영에 가까운 검증 checklist

Compose로 이지레이어를 띄우기 전 확인할 것:

- 이미지가 깨끗한 환경에서 `docker compose build --no-cache`로 빌드되는가
- 설정 파일이 이미지에 박혀 있지 않고 mount 또는 환경 변수로 분리되어 있는가
- bridge/host/macvlan 중 실제 packet path에 맞는 network mode를 선택했는가
- 필요한 capability만 추가했는가
- `privileged: true`가 정말 필요한지 설명할 수 있는가
- 로그와 pcap/debug output이 host로 빠져나오는가
- 컨테이너 종료 시 signal을 받고 정상 종료하는가
- CPU, hugepage, device 의존성이 README 또는 compose 주석으로 남아 있는가

## 11. 정리 명령

```bash
docker compose down
```

볼륨까지 지우려면:

```bash
docker compose down -v
```

이미지까지 다시 만들고 싶다면:

```bash
docker compose build --no-cache
docker compose up -d
```
