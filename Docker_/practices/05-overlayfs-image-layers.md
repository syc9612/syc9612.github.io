# 실습 05. OverlayFS와 이미지 레이어

이 실습은 Docker 이미지 layer와 컨테이너 writable layer가 어떻게 분리되는지 확인하고, OverlayFS의 `lowerdir`, `upperdir`, `workdir`, `merged`를 관찰한다.

## 목표

- 이미지 layer와 container writable layer의 차이를 확인한다.
- OverlayFS의 `lowerdir`, `upperdir`, `workdir`, `merged` 역할을 구분한다.
- 컨테이너 내부 파일 수정이 copy-on-write로 기록되는 흐름을 확인한다.
- Dockerfile 명령 순서가 build cache와 이미지 크기에 미치는 영향을 이해한다.
- 로그, pcap, 상태 파일을 writable layer가 아니라 volume으로 분리해야 하는 이유를 정리한다.

## 전제

Linux Docker 환경을 기준으로 한다.

Docker Desktop for Windows/macOS에서는 Docker daemon과 컨테이너 파일 시스템이 내부 Linux VM에 있으므로, Windows PowerShell 또는 macOS host 터미널에서 `/var/lib/docker` 경로가 직접 보이지 않을 수 있다. `docker diff`, `docker image history`, `docker inspect`는 Desktop에서도 확인 가능하지만, `UpperDir`, `MergedDir`의 실제 host 경로를 직접 열어보는 실습은 Linux VM, WSL2 내부 Linux, 또는 실제 Linux 서버에서 진행하는 편이 좋다.

일부 명령은 Docker root directory를 읽기 때문에 root 권한이 필요하다. 필요하면 `sudo`를 붙인다.

## 1. storage driver 확인

Docker가 사용하는 storage driver를 확인한다.

```bash
docker info --format '{{.Driver}}'
```

예상 출력:

```text
overlay2
```

추가 정보를 본다.

```bash
docker info --format '{{json .DriverStatus}}'
```

PowerShell에서도 같은 형식으로 확인할 수 있다.

```powershell
docker info --format '{{.Driver}}'
docker info --format '{{json .DriverStatus}}'
```

확인할 점:

| 값 | 의미 |
| --- | --- |
| `overlay2` | Docker의 일반적인 OverlayFS 기반 storage driver |
| `fuse-overlayfs` | rootless Docker에서 볼 수 있는 OverlayFS 계열 driver |
| 다른 값 | 환경별 driver가 다를 수 있으므로 이후 `GraphDriver` 출력을 기준으로 확인 |

## 2. 실습 이미지 준비

작업 디렉터리를 만든다.

```bash
mkdir -p overlay-lab
cd overlay-lab
```

테스트 파일을 만든다.

```bash
echo "version 1" > app.txt
```

Dockerfile을 만든다.

```bash
cat > Dockerfile <<'EOF'
FROM alpine:3.20

RUN mkdir -p /app && echo "base layer" > /app/base.txt
RUN echo "dependency layer" > /app/dependency.txt
COPY app.txt /app/app.txt
RUN echo "post copy layer" > /app/post-copy.txt

CMD ["sh", "-c", "ls -la /app && sleep 1d"]
EOF
```

PowerShell에서 같은 파일을 만들려면 다음처럼 작성한다.

```powershell
New-Item -ItemType Directory -Force overlay-lab
Set-Location overlay-lab
Set-Content -Encoding UTF8 app.txt "version 1"
@'
FROM alpine:3.20

RUN mkdir -p /app && echo "base layer" > /app/base.txt
RUN echo "dependency layer" > /app/dependency.txt
COPY app.txt /app/app.txt
RUN echo "post copy layer" > /app/post-copy.txt

CMD ["sh", "-c", "ls -la /app && sleep 1d"]
'@ | Set-Content -Encoding UTF8 Dockerfile
```

이미지를 빌드한다.

```bash
docker build -t overlay-lab:v1 .
```

## 3. 이미지 layer 확인

이미지 history를 확인한다.

```bash
docker image history overlay-lab:v1
```

예상 형태:

```text
IMAGE          CREATED          CREATED BY                                      SIZE
...            ...              RUN /bin/sh -c echo "post copy layer" ...       ...
...            ...              COPY app.txt /app/app.txt                      ...
...            ...              RUN /bin/sh -c echo "dependency layer" ...      ...
...            ...              RUN /bin/sh -c mkdir -p /app ...               ...
...            ...              Alpine base image layers                       ...
```

이미지의 root filesystem layer digest를 본다.

```bash
docker image inspect -f '{{range .RootFS.Layers}}{{println .}}{{end}}' overlay-lab:v1
```

의미:

| 출력 | 의미 |
| --- | --- |
| `sha256:...` 여러 줄 | 이미지가 여러 read-only layer로 구성됨 |
| `docker image history`의 각 줄 | Dockerfile 명령과 layer/cache 관계를 추적하는 단서 |

`RUN`, `COPY`, `ADD`처럼 파일 시스템을 바꾸는 명령은 layer를 만든다. `CMD`, `ENV`, `WORKDIR` 같은 metadata 명령은 이미지 config와 cache key에 영향을 줄 수 있지만, 파일 시스템 변경 layer를 항상 크게 만들지는 않는다.

## 4. build cache 영향 확인

소스 파일만 수정한다.

```bash
echo "version 2" > app.txt
docker build -t overlay-lab:v2 .
```

출력에서 확인할 점:

```text
RUN mkdir -p /app ...       CACHED
RUN echo "dependency..."    CACHED
COPY app.txt ...            다시 실행
RUN echo "post copy..."     다시 실행
```

`COPY app.txt`보다 앞에 있는 dependency layer는 재사용되고, `COPY app.txt`와 그 뒤의 layer는 다시 계산된다. 이 때문에 자주 바뀌지 않는 패키지 설치와 dependency build는 Dockerfile 앞쪽에 두고, 자주 바뀌는 애플리케이션 소스 복사는 뒤쪽에 두는 편이 좋다.

build context가 불필요하게 크면 cache invalidation 범위도 커진다. 보통 다음 파일을 함께 둔다.

```bash
cat > .dockerignore <<'EOF'
.git
build
logs
*.pcap
*.core
EOF
```

## 5. 컨테이너 writable layer 확인

컨테이너를 실행한다.

```bash
docker run -d --name overlay-lab overlay-lab:v1
```

컨테이너의 storage driver와 OverlayFS 경로를 확인한다.

```bash
docker inspect -f '{{.GraphDriver.Name}}' overlay-lab
docker inspect -f '{{json .GraphDriver.Data}}' overlay-lab
```

`overlay2` 환경에서는 다음과 비슷한 정보가 나온다.

```text
overlay2
{"LowerDir":"...","MergedDir":"...","UpperDir":"...","WorkDir":"..."}
```

각 경로의 의미:

| 경로 | 의미 |
| --- | --- |
| `LowerDir` | 이미지 read-only layer들이 연결된 경로 |
| `UpperDir` | 이 컨테이너에서 생긴 쓰기 변경사항 |
| `WorkDir` | OverlayFS 내부 작업 경로 |
| `MergedDir` | 컨테이너 프로세스가 실제로 보는 합성 결과 |

경로를 변수에 저장한다.

```bash
UPPER=$(docker inspect -f '{{.GraphDriver.Data.UpperDir}}' overlay-lab)
MERGED=$(docker inspect -f '{{.GraphDriver.Data.MergedDir}}' overlay-lab)
echo "$UPPER"
echo "$MERGED"
```

컨테이너를 아직 수정하지 않았다면 `UpperDir`은 비어 있거나 최소한의 파일만 있을 수 있다.

```bash
sudo find "$UPPER" -maxdepth 3 -print
```

## 6. copy-on-write 관찰

컨테이너 내부에서 기존 파일을 수정하고, 파일을 삭제하고, 새 파일을 만든다.

```bash
docker exec overlay-lab sh -c 'echo "runtime change" >> /app/base.txt'
docker exec overlay-lab sh -c 'rm /app/dependency.txt'
docker exec overlay-lab sh -c 'echo "runtime file" > /app/runtime.txt'
```

Docker 관점에서 변경사항을 확인한다.

```bash
docker diff overlay-lab
```

예상 형태:

```text
C /app
C /app/base.txt
D /app/dependency.txt
A /app/runtime.txt
```

의미:

| 표시 | 의미 |
| --- | --- |
| `C` | 변경됨 |
| `A` | 추가됨 |
| `D` | 삭제됨 |

`UpperDir`에서 실제 기록을 확인한다.

```bash
sudo find "$UPPER/app" -maxdepth 1 -print
sudo cat "$UPPER/app/base.txt"
sudo cat "$UPPER/app/runtime.txt"
sudo find "$UPPER/app" -name '.wh.*' -print
```

확인할 점:

- 수정한 `/app/base.txt`가 `UpperDir`에 복사되어 있다.
- 새로 만든 `/app/runtime.txt`도 `UpperDir`에 있다.
- 삭제한 `/app/dependency.txt`는 lower layer에서 지워진 것이 아니라 whiteout 항목으로 가려진다.

합성 결과는 `MergedDir`에서 확인할 수 있다.

```bash
sudo ls -la "$MERGED/app"
sudo cat "$MERGED/app/base.txt"
```

`MergedDir`은 컨테이너 프로세스가 보는 결과에 가깝고, `UpperDir`은 이 컨테이너에서 새로 생긴 변경분에 가깝다.

## 7. 이미지가 바뀌지 않는다는 점 확인

컨테이너를 삭제한다.

```bash
docker rm -f overlay-lab
```

같은 이미지로 새 컨테이너를 실행해서 원래 파일을 확인한다.

```bash
docker run --rm overlay-lab:v1 cat /app/base.txt
docker run --rm overlay-lab:v1 sh -c 'ls /app && test ! -e /app/runtime.txt'
```

예상 출력:

```text
base layer
```

이전 컨테이너에서 수정한 `runtime change`와 `runtime.txt`는 새 컨테이너에 남아 있지 않다. 변경사항은 이미지가 아니라 삭제된 컨테이너의 writable layer에 있었기 때문이다.

## 8. volume은 writable layer와 다르다

로그나 pcap 같은 실행 중 산출물은 컨테이너 writable layer에 쌓지 않는 편이 좋다. bind mount 또는 named volume으로 분리한다.

```bash
mkdir -p overlay-output
docker run --rm -v "$PWD/overlay-output:/out" overlay-lab:v1 sh -c 'echo "runtime output" > /out/result.txt'
cat overlay-output/result.txt
```

PowerShell에서는 현재 경로를 명시해 mount한다.

```powershell
New-Item -ItemType Directory -Force overlay-output
docker run --rm -v "${PWD}\overlay-output:/out" overlay-lab:v1 sh -c 'echo "runtime output" > /out/result.txt'
Get-Content .\overlay-output\result.txt
```

volume 또는 bind mount는 컨테이너 writable layer가 아니라 외부 storage에 기록된다. 컨테이너를 삭제해도 mount 대상에 기록된 파일은 남는다.

주의할 점:

- mount된 경로는 이미지 안의 같은 경로 내용을 가릴 수 있다.
- 운영 로그, pcap, debug dump, DB 파일은 writable layer보다 volume 또는 외부 저장소에 두는 편이 낫다.
- 비밀 정보와 설정 파일은 필요하면 read-only mount를 사용한다.

## 9. 이지레이어 이미지 최적화 관점

이지레이어 같은 C++ 기반 네트워크/패킷 처리 서비스는 빌드 의존성과 런타임 의존성을 분리해야 한다.

기본 방향:

```dockerfile
FROM ubuntu:24.04 AS build

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    cmake \
    ninja-build \
    pkg-config \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY CMakeLists.txt .
COPY cmake/ ./cmake/
COPY src/ ./src/

RUN cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release \
  && cmake --build build

FROM ubuntu:24.04 AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libstdc++6 \
  && rm -rf /var/lib/apt/lists/*

COPY --from=build /src/build/easylayer /usr/local/bin/easylayer
ENTRYPOINT ["easylayer"]
```

판단 기준:

| 항목 | 기준 |
| --- | --- |
| 빌드 도구 | runtime image에 남기지 않는다. |
| header/static library | 실행에 필요 없으면 runtime image에 복사하지 않는다. |
| config | image에 굽지 말고 read-only mount 또는 환경 변수로 주입한다. |
| logs/pcap | writable layer가 아니라 volume으로 분리한다. |
| dependency install | 소스 `COPY`보다 앞에 두어 cache를 재사용한다. |
| 실제 바이너리 경로 | 프로젝트 산출물 이름에 맞게 `COPY --from=build`를 수정한다. |

## 10. Troubleshooting

| 증상 | 확인할 내용 |
| --- | --- |
| `docker inspect`에 `UpperDir`이 보이지 않음 | storage driver 또는 Docker Desktop/Rootless/containerd image store 차이를 확인한다. |
| `/var/lib/docker` 경로가 없음 | Docker Desktop에서는 내부 Linux VM 기준 경로일 수 있다. |
| `sudo cat "$UPPER/..."` 권한 오류 | host에서 root 권한이 필요한지 확인한다. |
| `docker image history`의 size가 예상과 다름 | metadata-only instruction, base image layer, build cache 표시 방식을 구분한다. |
| 삭제한 파일이 `UpperDir`에 일반 파일로 보이지 않음 | OverlayFS whiteout 항목으로 표시될 수 있다. |
| build cache가 재사용되지 않음 | Dockerfile 명령 순서, build context 변경, `.dockerignore`, build arg 변경 여부를 확인한다. |

## 11. 정리

핵심 흐름:

```text
read-only image layers
  + container writable layer
  -> merged filesystem seen by container process
```

copy-on-write의 의미:

```text
lower layer file is not modified directly
first write copies the file to upper layer
runtime change belongs to the container, not to the image
```

Dockerfile 최적화 기준:

```text
stable dependency layers first
frequently changing source layers later
runtime image contains only what is needed to run
stateful output goes to volume
```

## 12. 정리 명령

실습 컨테이너와 이미지를 정리한다. 7장에서 컨테이너를 이미 삭제했다면 첫 줄은 건너뛰어도 된다.

```bash
docker rm -f overlay-lab
docker image rm overlay-lab:v1 overlay-lab:v2
```

실습 디렉터리까지 정리하려면 `overlay-lab`의 상위 디렉터리에서 실행한다.

```bash
rm -rf overlay-lab
```

PowerShell에서는 상위 디렉터리에서 다음처럼 정리한다.

```powershell
docker rm -f overlay-lab
docker image rm overlay-lab:v1 overlay-lab:v2
Remove-Item -Recurse -Force .\overlay-lab
```
