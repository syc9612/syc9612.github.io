# 실습 04. Container PID namespace 내부

이 실습은 Docker 컨테이너의 PID namespace를 확인하고, host PID와 container PID의 차이, PID 1의 역할, signal 처리, zombie process, `--init`, `--pid=host`의 의미를 관찰한다.

## 목표

- host에서 보이는 PID와 컨테이너 내부 PID가 다르다는 점을 확인한다.
- 컨테이너 내부 PID 1이 컨테이너 생명주기를 결정한다는 점을 확인한다.
- `docker top`, `docker exec`, `nsenter`가 각각 어떤 관점에서 프로세스를 보는지 구분한다.
- `docker stop`이 PID 1에 signal을 보내는 흐름을 확인한다.
- `--init`이 작은 init 프로세스를 PID 1로 넣는다는 점을 확인한다.
- `--pid=host`가 프로세스 격리를 약화한다는 점을 확인한다.

## 전제

Linux Docker 환경을 기준으로 한다.

Docker Desktop for Windows/macOS에서는 컨테이너가 내부 Linux VM에서 실행되므로, Windows PowerShell 또는 macOS host 터미널에서 `/proc/<pid>/ns/pid`, `nsenter`, host PID namespace를 그대로 관찰하기 어렵다. 이 실습은 Linux VM, WSL2 내부 Linux, 또는 실제 Linux 서버에서 진행하는 편이 좋다.

명령 중 일부는 root 권한이 필요하다. 필요하면 `sudo`를 붙인다.

## 1. 테스트 컨테이너 실행

간단히 오래 살아 있는 컨테이너를 하나 실행한다.

```bash
docker run -d --name pid-lab alpine sleep 1d
```

확인:

```bash
docker ps --filter name=pid-lab
```

기대하는 상태:

```text
pid-lab 컨테이너가 Up 상태
```

## 2. host PID와 container PID 비교

host에서 컨테이너의 실제 PID를 확인한다.

```bash
HOST_PID=$(docker inspect -f '{{.State.Pid}}' pid-lab)
echo "$HOST_PID"
```

PowerShell에서는 다음처럼 저장한다.

```powershell
$HOST_PID = docker inspect -f '{{.State.Pid}}' pid-lab
$HOST_PID
```

host PID namespace에서 해당 프로세스를 본다.

```bash
ps -o pid,ppid,stat,cmd -p "$HOST_PID"
```

예상 형태:

```text
    PID    PPID STAT CMD
  12345    9876 Ss   sleep 1d
```

컨테이너 내부에서 같은 프로세스를 본다.

```bash
docker exec pid-lab ps
```

예상 형태:

```text
PID   USER     TIME  COMMAND
    1 root      0:00 sleep 1d
```

같은 `sleep` 프로세스지만 host에서는 `12345` 같은 host PID로 보이고, 컨테이너 내부에서는 PID `1`로 보인다.

## 3. PID namespace inode 확인

host 자신의 PID namespace를 확인한다.

```bash
readlink /proc/self/ns/pid
```

컨테이너 프로세스가 속한 PID namespace를 host에서 확인한다.

```bash
readlink /proc/"$HOST_PID"/ns/pid
```

컨테이너 내부에서 PID 1의 PID namespace를 확인한다.

```bash
docker exec pid-lab readlink /proc/1/ns/pid
```

확인할 점:

| 비교 | 의미 |
| --- | --- |
| `/proc/self/ns/pid` | host shell의 PID namespace |
| `/proc/$HOST_PID/ns/pid` | 컨테이너 프로세스의 PID namespace |
| 컨테이너 내부 `/proc/1/ns/pid` | 컨테이너 내부에서 본 PID namespace |

host의 `/proc/$HOST_PID/ns/pid`와 컨테이너 내부 `/proc/1/ns/pid`가 같은 inode이면 같은 PID namespace를 가리킨다.

## 4. `docker top`으로 host 관점 확인

```bash
docker top pid-lab
```

예상 형태:

```text
UID    PID      PPID     C    STIME    TTY    TIME       CMD
root   12345    9876     0    12:00    ?      00:00:00   sleep 1d
```

`docker top`은 컨테이너 내부 PID가 아니라 host PID namespace 기준의 PID를 보여준다. `docker exec pid-lab ps` 결과와 숫자가 다르게 보이는 것이 정상이다.

## 5. `nsenter`로 PID namespace 진입

host에서 컨테이너의 PID namespace로 들어가 프로세스를 확인한다.

```bash
sudo nsenter --target "$HOST_PID" --mount --pid --root --wd --mount-proc ps
```

예상 형태:

```text
PID   USER     TIME  COMMAND
    1 root      0:00 sleep 1d
```

의미:

| 옵션 | 의미 |
| --- | --- |
| `--target "$HOST_PID"` | 이 프로세스의 namespace를 기준으로 진입 |
| `--pid` | PID namespace 진입 |
| `--mount` | mount namespace 진입 |
| `--root` | target process의 root directory 사용 |
| `--wd` | target process의 working directory 사용 |
| `--mount-proc` | 진입한 PID namespace 기준으로 `/proc` mount |

PID namespace만 바꾸고 host의 `/proc`를 그대로 보면 `ps` 출력이 혼동될 수 있다. 그래서 process tree를 볼 때는 mount namespace와 `/proc`도 함께 맞추는 편이 좋다.

`nsenter` 버전이 `--root`, `--wd`, `--mount-proc`를 지원하지 않으면 다음처럼 컨테이너 내부 명령으로 대체한다.

```bash
docker exec pid-lab ps
```

## 6. PID 1 종료와 컨테이너 생명주기

PID 1이 종료되면 컨테이너도 종료된다.

```bash
docker run -d --name pid-exit-lab alpine sh -c 'sleep 3'
docker ps --filter name=pid-exit-lab
sleep 5
docker ps -a --filter name=pid-exit-lab
```

기대하는 상태:

```text
pid-exit-lab 컨테이너가 Exited 상태
```

컨테이너는 VM이 아니라 프로세스 격리 실행 단위다. PID 1로 실행한 command가 끝나면 컨테이너의 주 실행 프로세스가 끝난 것이므로 컨테이너도 종료된다.

## 7. `docker stop`과 signal 처리 확인

`docker stop`은 기본적으로 컨테이너 PID 1에 `SIGTERM`을 보내고, 제한 시간 안에 종료되지 않으면 `SIGKILL`을 보낸다.

signal을 처리하는 컨테이너를 실행한다.

```bash
docker run -d --name signal-lab alpine sh -c 'trap "echo got SIGTERM; exit 0" TERM; while true; do sleep 1; done'
```

컨테이너를 종료한다.

```bash
docker stop -t 5 signal-lab
docker logs signal-lab
```

예상 출력:

```text
got SIGTERM
```

이 예시는 PID 1이 `SIGTERM`을 받아 정상 종료 로직을 실행하는 흐름을 보여준다. 실제 애플리케이션에서는 이 시점에 connection drain, 로그 flush, 임시 파일 정리 같은 작업을 수행해야 한다.

## 8. `--init`으로 작은 init 프로세스 넣기

`--init`을 사용하면 Docker가 작은 init 프로세스를 PID 1로 넣는다.

```bash
docker run -d --init --name init-lab alpine sleep 1d
docker exec init-lab ps
```

예상 형태:

```text
PID   USER     TIME  COMMAND
    1 root      0:00 /sbin/docker-init -- sleep 1d
    7 root      0:00 sleep 1d
```

`docker top`으로 host 기준에서도 확인한다.

```bash
docker top init-lab
```

`--init`은 signal forwarding과 orphan child 회수에 도움을 준다. child process를 만들거나 shell wrapper를 쓰는 컨테이너에서는 기본값으로 검토할 만하다.

Compose에서는 다음 옵션이 같은 역할을 한다.

```yaml
services:
  app:
    image: alpine
    init: true
    command: sleep 1d
```

주의할 점:

```text
--init은 애플리케이션의 모든 프로세스 관리 버그를 대신 고쳐주지 않는다.
애플리케이션이 직접 만든 child process를 계속 wait하지 않는 경우는 애플리케이션 쪽 수정이 필요하다.
```

## 9. zombie process 관찰

부모가 child process 종료 상태를 회수하지 않으면 zombie process가 남을 수 있다. 관찰을 위해 Python 예시 컨테이너를 사용한다.

```bash
docker run -d --name zombie-lab python:3.12-alpine \
  python -c 'import os,time; pid=os.fork(); os._exit(0) if pid == 0 else (print("parent", os.getpid(), "child", pid, flush=True), time.sleep(3600))'
```

컨테이너 로그를 확인한다.

```bash
docker logs zombie-lab
```

프로세스 상태를 `/proc`에서 확인한다.

```bash
docker exec zombie-lab sh -c 'for p in /proc/[0-9]*; do awk "/^Name:|^State:|^Pid:|^PPid:/ {print}" "$p/status"; echo; done'
```

찾아볼 상태:

```text
State:  Z (zombie)
```

이 상태는 child process가 이미 종료됐지만 parent process가 `wait` 계열 호출로 종료 상태를 회수하지 않았다는 뜻이다.

정리:

| 상황 | 대응 |
| --- | --- |
| 애플리케이션이 직접 child를 만들고 회수하지 않음 | 애플리케이션 코드에서 `wait` 처리 필요 |
| parent가 죽어 child가 orphan이 됨 | PID 1 또는 `--init` init 프로세스가 회수 |
| shell wrapper가 child를 방치함 | `exec` 사용 또는 `init: true` 검토 |

## 10. `--pid=host` 확인

기본 컨테이너는 host PID namespace와 분리되어 있다. `--pid=host`를 사용하면 컨테이너가 host PID namespace를 공유한다.

```bash
docker run --rm --pid=host alpine ps
```

확인할 점:

```text
컨테이너 내부 ps에서 host 프로세스들이 보인다.
```

이 방식은 디버깅 또는 모니터링 에이전트에는 유용할 수 있지만, 컨테이너 프로세스 격리가 약해진다. 운영 환경에서는 필요한 이유가 명확해야 한다.

다른 컨테이너의 PID namespace에 붙는 방식도 있다.

```bash
docker run --rm --pid=container:pid-lab alpine ps
```

이 방식은 sidecar 디버깅이나 Kubernetes Pod 구조를 이해할 때 도움이 된다.

## 11. Troubleshooting

| 증상 | 확인할 내용 |
| --- | --- |
| `nsenter: command not found` | host에 `util-linux` 패키지가 설치되어 있는지 확인한다. |
| `nsenter` 실행 시 권한 오류 | root 권한 또는 `sudo` 사용 여부를 확인한다. |
| `ps` 출력이 host 기준으로 보임 | PID namespace만 들어가고 `/proc`가 host 기준으로 남아 있을 수 있다. |
| Docker Desktop에서 host PID가 이상하게 보임 | Docker daemon이 내부 Linux VM에 있으므로 VM 내부에서 실습해야 한다. |
| `--init` 출력에 `/sbin/docker-init`이 보이지 않음 | Docker 버전과 runtime 설정을 확인한다. |

## 12. 정리

핵심 흐름:

```text
host PID namespace
  sees container process as host PID 12345

container PID namespace
  sees same process as PID 1
```

컨테이너 PID 1의 핵심 책임:

```text
signal handling
child process reaping
container lifecycle ownership
```

`--pid=host`의 핵심 trade-off:

```text
host process visibility increases
process isolation decreases
```

## 13. 정리 명령

실습이 끝나면 생성한 컨테이너를 정리한다.

```bash
docker rm -f pid-lab pid-exit-lab signal-lab init-lab zombie-lab
```
