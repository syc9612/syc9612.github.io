# 실습 06. Docker Compose에서 Kubernetes까지 연결

이 실습은 Docker Compose로 표현하던 서비스, 네트워크, 설정, 볼륨, healthcheck를 Kubernetes 리소스로 옮길 때 어떤 개념으로 대응되는지 확인한다.

실제 이지레이어 이미지를 바로 사용하기 전, 검증 가능한 `nginx` 기반 control API 샘플로 Kubernetes 리소스의 형태를 먼저 확인한다. 이후 이지레이어 이미지, config 경로, capability, device 요구사항에 맞게 manifest를 바꾸는 흐름으로 정리한다.

## 목표

- Compose service가 Kubernetes의 Pod, Deployment, Service로 나뉘는 흐름을 확인한다.
- ConfigMap, Secret, Volume mount로 설정을 image 밖에 두는 방식을 확인한다.
- Compose `healthcheck`가 Kubernetes probe로 분리되는 방식을 확인한다.
- Docker bridge와 Kubernetes Pod network, Service, kube-proxy/CNI의 역할 차이를 구분한다.
- 이지레이어처럼 packet path가 중요한 서비스를 DaemonSet, `hostNetwork`, `securityContext`, device plugin 관점으로 옮기는 기준을 정리한다.

## 전제

`kubectl`이 설치되어 있고, 접근 가능한 Kubernetes cluster가 있다고 가정한다. Docker Desktop Kubernetes, kind, minikube, 원격 개발 cluster 중 어느 쪽이든 상관없지만, node network와 device를 직접 보는 항목은 실제 Linux node에서만 의미가 있다.

현재 context를 확인한다.

```bash
kubectl config current-context
kubectl get nodes -o wide
```

PowerShell에서도 같은 명령을 사용한다.

```powershell
kubectl config current-context
kubectl get nodes -o wide
```

주의:

- local Docker에서 `docker build -t easylayer:local .`로 만든 이미지는 원격 Kubernetes node에서 자동으로 보이지 않는다.
- kind, minikube, Docker Desktop처럼 local cluster를 쓰면 image load 방식이 cluster마다 다르다.
- 운영 cluster에서는 image registry에 push하고 `imagePullSecrets` 또는 registry 인증을 따로 구성한다.

## 1. Compose와 Kubernetes 리소스 대응

예를 들어 Compose에서는 다음처럼 한 서비스 안에 실행, 포트, 설정, volume, healthcheck가 함께 들어간다.

```yaml
services:
  easylayer:
    image: easylayer:local
    environment:
      EASYLAYER_CONFIG: /etc/easylayer/easylayer.yaml
    volumes:
      - ./config:/etc/easylayer:ro
      - ./logs:/var/log/easylayer
    ports:
      - "8080:8080"
    healthcheck:
      test: ["CMD-SHELL", "curl -fsS http://localhost:8080/healthz || exit 1"]
```

Kubernetes에서는 같은 의도를 여러 리소스로 분리한다.

| Compose 항목 | Kubernetes 리소스/필드 |
| --- | --- |
| `services.easylayer.image` | Deployment `.spec.template.spec.containers[].image` |
| `environment` | `env`, ConfigMap, Secret |
| bind mount config | ConfigMap volume 또는 Secret volume |
| bind mount logs | `emptyDir`, PersistentVolumeClaim, hostPath, log collector |
| `ports` | Service, Ingress, Gateway, NodePort |
| `healthcheck` | `readinessProbe`, `livenessProbe`, `startupProbe` |
| `cap_add`, `devices` | `securityContext`, device plugin, resource request |

핵심은 “Compose service 하나가 Kubernetes object 하나로만 바뀌지 않는다”는 점이다.

## 2. namespace 준비

실습용 namespace를 만든다.

```bash
kubectl create namespace docker-k8s-lab
```

이미 있으면 다음처럼 확인만 한다.

```bash
kubectl get namespace docker-k8s-lab
```

## 3. ConfigMap과 Secret 생성

비밀이 아닌 설정은 ConfigMap으로 둔다.

```bash
kubectl create configmap easylayer-config \
  --namespace docker-k8s-lab \
  --from-literal=easylayer.yaml='log_level: info'
```

PowerShell에서는 한 줄로 실행하는 편이 단순하다.

```powershell
kubectl create configmap easylayer-config --namespace docker-k8s-lab --from-literal=easylayer.yaml='log_level: info'
```

민감한 값은 Secret으로 둔다. 이 예시는 실습용 값이다.

```bash
kubectl create secret generic easylayer-secret \
  --namespace docker-k8s-lab \
  --from-literal=api-token=dev-token
```

PowerShell:

```powershell
kubectl create secret generic easylayer-secret --namespace docker-k8s-lab --from-literal=api-token=dev-token
```

확인:

```bash
kubectl get configmap,secret -n docker-k8s-lab
kubectl describe configmap easylayer-config -n docker-k8s-lab
```

주의:

- Secret은 ConfigMap보다 민감 정보용 API object지만, cluster의 etcd 암호화와 RBAC 설정을 별도로 확인해야 한다.
- ConfigMap과 Secret은 같은 namespace의 Pod에서 참조한다.
- 큰 설정 파일이나 자주 바뀌는 룰셋은 ConfigMap 크기 제한과 rollout 전략을 함께 고려한다.

## 4. Deployment와 Service 작성

먼저 실제 cluster에서 검증하기 쉬운 `nginx` 기반 control API 샘플을 만든다.

```bash
cat > k8s-control-sample.yaml <<'EOF'
apiVersion: apps/v1
kind: Deployment
metadata:
  name: easylayer-control-sample
  namespace: docker-k8s-lab
  labels:
    app: easylayer-control-sample
spec:
  replicas: 2
  selector:
    matchLabels:
      app: easylayer-control-sample
  template:
    metadata:
      labels:
        app: easylayer-control-sample
    spec:
      containers:
      - name: control
        image: nginx:1.27-alpine
        ports:
        - containerPort: 80
          name: http
        env:
        - name: EASYLAYER_CONFIG
          value: /etc/easylayer/easylayer.yaml
        - name: EASYLAYER_API_TOKEN
          valueFrom:
            secretKeyRef:
              name: easylayer-secret
              key: api-token
        volumeMounts:
        - name: config
          mountPath: /etc/easylayer
          readOnly: true
        readinessProbe:
          httpGet:
            path: /
            port: http
          periodSeconds: 5
          timeoutSeconds: 2
        livenessProbe:
          httpGet:
            path: /
            port: http
          periodSeconds: 10
          timeoutSeconds: 2
      volumes:
      - name: config
        configMap:
          name: easylayer-config
---
apiVersion: v1
kind: Service
metadata:
  name: easylayer-control-sample
  namespace: docker-k8s-lab
spec:
  type: ClusterIP
  selector:
    app: easylayer-control-sample
  ports:
  - name: http
    protocol: TCP
    port: 8080
    targetPort: http
EOF
```

PowerShell에서는 here-string을 사용한다.

```powershell
@'
apiVersion: apps/v1
kind: Deployment
metadata:
  name: easylayer-control-sample
  namespace: docker-k8s-lab
  labels:
    app: easylayer-control-sample
spec:
  replicas: 2
  selector:
    matchLabels:
      app: easylayer-control-sample
  template:
    metadata:
      labels:
        app: easylayer-control-sample
    spec:
      containers:
      - name: control
        image: nginx:1.27-alpine
        ports:
        - containerPort: 80
          name: http
        env:
        - name: EASYLAYER_CONFIG
          value: /etc/easylayer/easylayer.yaml
        - name: EASYLAYER_API_TOKEN
          valueFrom:
            secretKeyRef:
              name: easylayer-secret
              key: api-token
        volumeMounts:
        - name: config
          mountPath: /etc/easylayer
          readOnly: true
        readinessProbe:
          httpGet:
            path: /
            port: http
          periodSeconds: 5
          timeoutSeconds: 2
        livenessProbe:
          httpGet:
            path: /
            port: http
          periodSeconds: 10
          timeoutSeconds: 2
      volumes:
      - name: config
        configMap:
          name: easylayer-config
---
apiVersion: v1
kind: Service
metadata:
  name: easylayer-control-sample
  namespace: docker-k8s-lab
spec:
  type: ClusterIP
  selector:
    app: easylayer-control-sample
  ports:
  - name: http
    protocol: TCP
    port: 8080
    targetPort: http
'@ | Set-Content -Encoding UTF8 k8s-control-sample.yaml
```

적용한다.

```bash
kubectl apply -f k8s-control-sample.yaml
```

확인:

```bash
kubectl get deployment,pod,service,endpointslice -n docker-k8s-lab
kubectl rollout status deployment/easylayer-control-sample -n docker-k8s-lab
```

예상 형태:

```text
deployment.apps/easylayer-control-sample created
service/easylayer-control-sample created
deployment "easylayer-control-sample" successfully rolled out
```

## 5. Pod, Service, EndpointSlice 관찰

Pod IP와 node 배치를 확인한다.

```bash
kubectl get pods -n docker-k8s-lab -o wide
```

Service가 바라보는 endpoint를 확인한다.

```bash
kubectl get service easylayer-control-sample -n docker-k8s-lab -o wide
kubectl get endpointslice -n docker-k8s-lab -l kubernetes.io/service-name=easylayer-control-sample -o wide
```

확인할 점:

| 항목 | 의미 |
| --- | --- |
| Pod IP | Kubernetes Pod network에서 할당된 IP |
| Service ClusterIP | client가 안정적으로 바라보는 cluster 내부 IP |
| EndpointSlice address | Service 뒤에 붙은 실제 Pod endpoint |
| selector label | Service가 어떤 Pod를 backend로 선택하는지 결정 |

Docker Compose의 service name DNS와 비슷하게 보일 수 있지만, Kubernetes에서는 Service와 EndpointSlice가 변하는 Pod 집합을 추상화한다는 점이 중요하다.

## 6. cluster 내부 접근 확인

임시 client Pod에서 Service DNS로 접근한다.

```bash
kubectl run curl-client \
  --namespace docker-k8s-lab \
  --rm -it \
  --image=curlimages/curl:8.10.1 \
  --restart=Never \
  -- curl -fsS http://easylayer-control-sample:8080/
```

PowerShell:

```powershell
kubectl run curl-client --namespace docker-k8s-lab --rm -it --image=curlimages/curl:8.10.1 --restart=Never -- curl -fsS http://easylayer-control-sample:8080/
```

예상 결과:

```text
nginx 기본 HTML 응답 일부가 출력된다.
```

local machine에서 확인하려면 port-forward를 사용한다.

```bash
kubectl port-forward -n docker-k8s-lab service/easylayer-control-sample 8080:8080
```

다른 터미널에서 확인한다.

```bash
curl -fsS http://localhost:8080/
```

PowerShell에서는 다음처럼 확인할 수 있다.

```powershell
Invoke-WebRequest http://localhost:8080/
```

## 7. probe 상태 확인

Pod의 Ready 상태와 event를 본다.

```bash
kubectl get pods -n docker-k8s-lab
kubectl describe pod -n docker-k8s-lab -l app=easylayer-control-sample
```

확인할 점:

| 항목 | 의미 |
| --- | --- |
| `READY 1/1` | readinessProbe가 성공해 Service endpoint로 들어갈 수 있음 |
| `Liveness` | kubelet이 컨테이너 생존 여부를 주기적으로 확인 |
| `Readiness` | kubelet이 traffic 수신 가능 여부를 주기적으로 확인 |
| Events | image pull, scheduling, probe 실패 같은 원인 확인 |

Compose `healthcheck` 하나로 표현하던 것을 Kubernetes에서는 startup, readiness, liveness 의도별로 나눠야 한다.

## 8. 이지레이어 control API로 바꾸는 지점

위 샘플을 실제 이지레이어로 바꿀 때는 다음 항목을 수정한다.

```yaml
containers:
- name: easylayer
  image: registry.example.com/easylayer:1.0.0
  command: ["easylayer"]
  args: ["--config", "/etc/easylayer/easylayer.yaml"]
  ports:
  - containerPort: 8080
    name: http
  readinessProbe:
    httpGet:
      path: /healthz
      port: http
  livenessProbe:
    httpGet:
      path: /livez
      port: http
```

확인할 내용:

- `image`가 모든 node에서 pull 가능한 registry에 있는가
- 실제 binary 이름과 argument가 맞는가
- `/etc/easylayer/easylayer.yaml` 경로가 애플리케이션의 실제 config 경로와 맞는가
- control API가 readiness와 liveness를 구분해 제공하는가
- 로그, pcap, debug output은 container writable layer가 아니라 volume 또는 logging pipeline으로 빠지는가

## 9. DaemonSet과 host network 템플릿

packet capture, raw socket, host NIC 접근처럼 node-local packet path가 중요하면 Deployment보다 DaemonSet이 자연스러울 수 있다.

아래 manifest는 바로 운영에 쓰는 예시가 아니라 구조 확인용 템플릿이다. 실제 capability와 mount는 이지레이어의 packet I/O 방식이 확정된 뒤 최소화해야 한다.

```bash
cat > k8s-easylayer-daemonset-template.yaml <<'EOF'
apiVersion: apps/v1
kind: DaemonSet
metadata:
  name: easylayer-agent
  namespace: docker-k8s-lab
  labels:
    app: easylayer-agent
spec:
  selector:
    matchLabels:
      app: easylayer-agent
  template:
    metadata:
      labels:
        app: easylayer-agent
    spec:
      hostNetwork: true
      dnsPolicy: ClusterFirstWithHostNet
      containers:
      - name: easylayer
        image: registry.example.com/easylayer:1.0.0
        args: ["--config", "/etc/easylayer/easylayer.yaml"]
        securityContext:
          allowPrivilegeEscalation: false
          capabilities:
            drop: ["ALL"]
            add: ["NET_RAW", "NET_ADMIN"]
        volumeMounts:
        - name: config
          mountPath: /etc/easylayer
          readOnly: true
        - name: host-logs
          mountPath: /host/var/log
          readOnly: true
      volumes:
      - name: config
        configMap:
          name: easylayer-config
      - name: host-logs
        hostPath:
          path: /var/log
          type: Directory
EOF
```

server-side dry run으로 API schema를 확인한다.

```bash
kubectl apply --dry-run=server -f k8s-easylayer-daemonset-template.yaml
```

주의:

- `hostNetwork: true`는 Pod가 node의 network namespace를 공유하므로 port 충돌과 격리 약화가 생긴다.
- `NET_RAW`, `NET_ADMIN`은 예시일 뿐이며 실제 필요한 기능과 syscall 기준으로 줄여야 한다.
- `hostPath`는 host filesystem을 노출하므로 read-only를 우선하고, 경로와 목적을 문서화해야 한다.
- DPDK, AF_XDP, SR-IOV, GPU, FPGA 같은 장치는 보통 device plugin 또는 vendor plugin을 검토한다.

## 10. kube-proxy, CNI, Service data path 확인

cluster의 CNI와 kube-proxy 상태는 환경마다 다르다.

```bash
kubectl get pods -A -o wide | grep -E 'cni|calico|cilium|flannel|weave|kube-proxy'
```

PowerShell에서는 다음처럼 필터링한다.

```powershell
kubectl get pods -A -o wide | Select-String -Pattern 'cni|calico|cilium|flannel|weave|kube-proxy'
```

확인할 점:

| 항목 | 의미 |
| --- | --- |
| CNI plugin | Pod IP 할당과 Pod 간 network model 구현 |
| kube-proxy | Service virtual IP와 backend forwarding 구현 |
| EndpointSlice | Service가 바라보는 backend Pod endpoint |
| NetworkPolicy plugin | Pod 간 접근 제어를 실제로 적용하는 plugin |

일부 CNI는 kube-proxy를 대체하거나 eBPF 기반 data path를 제공한다. 이 경우 iptables/IPVS 규칙이 적게 보일 수 있으므로 해당 CNI 문서를 함께 확인해야 한다.

## 11. Troubleshooting

| 증상 | 확인할 내용 |
| --- | --- |
| `kubectl`이 cluster에 연결되지 않음 | `kubectl config current-context`, kubeconfig, VPN, cluster endpoint 확인 |
| Pod가 `ImagePullBackOff` | image 이름, tag, registry 인증, local image load 여부 확인 |
| Pod가 `CrashLoopBackOff` | `kubectl logs`, command/args, config 경로, secret mount 확인 |
| Service로 접근 안 됨 | Service selector와 Pod label 일치 여부, EndpointSlice 생성 여부 확인 |
| readiness가 계속 실패 | probe path, port name, app 초기화 시간, config load 상태 확인 |
| DaemonSet Pod가 안 뜸 | nodeSelector, taint/toleration, security policy, image pull 확인 |
| `hostNetwork`에서 DNS가 이상함 | `dnsPolicy: ClusterFirstWithHostNet` 필요 여부 확인 |
| `hostPath` mount 실패 | node에 실제 경로가 있는지, type이 맞는지, 보안 정책에 막히지 않는지 확인 |

## 12. 정리

핵심 대응:

```text
Compose service
  -> Deployment or DaemonSet
  -> Pod template
  -> Service
  -> ConfigMap / Secret / Volume
  -> Probe / securityContext / resource requests
```

네트워크 관점:

```text
Docker bridge network
  -> Kubernetes Pod network by CNI
  -> stable access through Service
  -> backend tracking through EndpointSlice
```

이지레이어 판단:

```text
control API: Deployment + ClusterIP Service
node packet path: DaemonSet + hostNetwork/device/securityContext review
state/config/log: ConfigMap, Secret, Volume, external logging
```

## 13. 정리 명령

실습 리소스를 지운다.

```bash
kubectl delete -f k8s-control-sample.yaml
kubectl delete configmap easylayer-config -n docker-k8s-lab
kubectl delete secret easylayer-secret -n docker-k8s-lab
kubectl delete namespace docker-k8s-lab
```

템플릿 파일은 local 파일이므로 필요 없으면 삭제한다.

```bash
rm -f k8s-control-sample.yaml k8s-easylayer-daemonset-template.yaml
```

PowerShell에서는 다음처럼 삭제한다.

```powershell
Remove-Item -Force .\k8s-control-sample.yaml, .\k8s-easylayer-daemonset-template.yaml
```

## 14. 참고 reference

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
