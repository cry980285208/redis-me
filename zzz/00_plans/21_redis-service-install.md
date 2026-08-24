# 21. Redis 服务安装帮助（Docker 快速搭建）

> **类型**：设计草案 + 讨论清单（未定稿）  
> **关联**：[`zzz/01_redis-install`](../01_redis-install/)（现有手工 compose 样例，即本功能的模板来源）  
> **关联组件**：`MeCode.vue`（CodeMirror 输出区）、`MeDialog.vue`、`locales`  
> **日期**：2026-08-22

---

## 一、目标与定位

在 RedisME 内提供「Redis 安装帮助」：用户填表单 → **界面内用 CodeMirror 生成** docker 命令 / docker-compose.yaml / sentinel.conf / SSL 证书生成命令 → 用户复制到目标 **Linux 服务器**上执行。

**核心目标：快速搭建集群**（尤其是三主三从一键式）。

### 定位三原则（钉死）

| 原则      | 说明                                                                                 |
| --------- | ------------------------------------------------------------------------------------ |
| 只生成    | RedisME **不执行**任何 docker 命令、不落盘用户环境；产物是纯文本，复制/保存即走      |
| 仅 Docker | 不做裸机编译安装、不做 systemd、不做 K8s；覆盖 `docker run` 与 `docker compose` 两种 |
| 仅 Linux  | 生成的脚本/网络模式按 Linux 假设（host 网络等）；Windows/macOS 目标机不在支持范围    |

---

## 二、范围决策（初步）

| 项           | 结论                                                                                                                                                                        |
| ------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 部署模式     | 单机 / 集群（cluster）/ 哨兵（sentinel）三种                                                                                                                                |
| 快速模板     | 集群默认 **三主三从**；哨兵默认 **一主二从 + 三哨兵**；节点数可调                                                                                                           |
| 镜像         | `redis:8`（默认）/ `redis:7` / `valkey:8`（或自定义镜像名）                                                                                                                 |
| 安装方式输出 | ① 原始 docker 命令（`docker run` 逐条）② docker compose（yaml + 附属 conf）③ **分步指南**：多个步骤合并在一个代码块展示，每步带注释，用户自行逐步复制执行（已定，见 §五.3） |
| 持久化       | **默认开启**（已定，见 §五.9）：数据目录 `./data-<port>:/data` + `appendonly yes`（写在 conf 内）；可手动关闭                                                               |
| 配置文件     | **默认外置挂载**（已定，见 §五.16）：每节点生成独立 `redis.conf` 挂载，启动命令仅一行；避免 command 过长，方便用户追加 `rename-command` 等定制配置                          |
| 端口         | 基准端口 + 自动递增；集群总线端口 = port+10000 需提示放行                                                                                                                   |
| 密码         | `requirepass`；集群/哨兵自动同步 `masterauth`、哨兵 `auth-pass`（三者必须一致）                                                                                             |
| SSL          | 可选，**双模式**：① **应用内签发**（用户填必要信息 + **选密钥类型**，后端生成 CA+服务器证书 PEM，已定可行，见 §五.11）② 生成 **openssl 命令**（保留）；均配 `--tls-*` 参数  |
| announce-ip  | 集群/哨兵必填项；**单输入框支持分号分隔多 IP**（单机=1 个，多机=多个，自动分割去重，已定，见 §五.12）；填宿主机可达 IP（最大踩坑点，见 §五.2）                              |
| i18n         | 中英双语文案，走 `locales`                                                                                                                                                  |

---

## 三、功能设计

### 3.1 入口与界面布局

- 入口：**主界面独立「工具箱」入口**（已定；与连接管理解耦，不在连接页加引导）
- 布局：左侧表单区（模式、镜像、IP/端口、密码、持久化、SSL 开关），右侧 **CodeMirror 只读输出区**（复用 `MeCode`，与导出/命令预览一致的暗色风格）
- 输出区顶部 tab 切换：**docker 命令** / **docker compose** / **分步指南**（所有步骤合并一个代码块，注释标注每步）
- 官方链接（已定 ✅）：页面提供两处入口——[Docker Hub 官方镜像页](https://hub.docker.com/_/redis)、[Redis 官方 Docker 安装手册](https://redis.io/docs/latest/operate/oss_and_stack/install/install-stack/docker/)；按项目外链规范统一 `@click` + `meOpenUrl` 打开（禁止 href）
- 操作按钮：复制（剪贴板）、保存文件（Tauri save dialog；多文件时按 §五.4 方案处理）
- 生成后附「连接摘要」：host / port / password / SSL 一览，方便用户装完后回 RedisME 建连接（见 §五.8）

### 3.2 表单字段（三种模式共用 + 差异）

| 字段         | 单机 | 集群 | 哨兵 | 备注                                                                                                                                     |
| ------------ | ---- | ---- | ---- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| 镜像/版本    | ✅   | ✅   | ✅   | 下拉 + 自定义输入                                                                                                                        |
| 基础端口     | ✅   | ✅   | ✅   | 集群默认 7001 起；哨兵 redis 7701 / sentinel 27701 规则递增                                                                              |
| requirepass  | ✅   | ✅   | ✅   | 空则不启用认证                                                                                                                           |
| announce-ip  | —    | ✅   | ✅   | 单输入框、分号分隔多 IP（多机，见 §五.12）；集群 `--cluster-announce-ip`；哨兵 `--replica-announce-ip` + `sentinel announce-ip`          |
| 主/从节点数  | —    | ✅   | ✅   | 集群默认 3 主 3 从；哨兵固定一主二从（从数可调）                                                                                         |
| 哨兵数量     | —    | —    | ✅   | 默认 3                                                                                                                                   |
| 数据持久化   | ✅   | ✅   | ✅   | 开关；开启后追加 volume + appendonly                                                                                                     |
| 配置文件挂载 | ✅   | ✅   | ✅   | **默认开启**（已定，见 §五.16）：每节点独立 `redis.conf` 挂载；哨兵 `sentinel.conf` 必挂载（有状态，§六.3）；可关闭退回命令行参数形态    |
| SSL          | ✅   | ✅   | ✅   | 开启后追加 tls 参数 + 证书生成命令；集群需 `--tls-replication yes --tls-cluster yes`；哨兵需 `tls-replication yes` + sentinel `tls-port` |
| 网络模式     | ✅   | ✅   | ✅   | 默认 `host`（Linux 场景最省事）；可选 bridge + 端口映射                                                                                  |
| 时区         | ✅   | ✅   | ✅   | 默认 `TZ=Asia/Shanghai`，可改                                                                                                            |
| restart 策略 | ✅   | ✅   | ✅   | 默认 `always`                                                                                                                            |

### 3.3 生成逻辑要点（对齐 `01_redis-install` 现有样例）

- **单机**：对齐 `redis-single` / `redis-single-ssl`；compose/分步指南默认用挂载 `redis.conf` 形态（对齐 `docker-compose-config.yaml`），docker 命令 tab 保留命令行参数形态（一条即跑）
- **集群**：对齐 `redis-cluster` / `redis-cluster-ssl`；每节点 `redis.conf` 含 `cluster-enabled yes` 等；**额外生成 `redis-cli --cluster create` 命令**：
  ```text
  redis-cli --cluster create <ip>:7001 ... <ip>:7006 --cluster-replicas 1 --cluster-yes -a <密码>
  ```
  （现有样例缺失此关键一步，是"快速搭建"的核心补全）
- **哨兵**：对齐 `redis-sentinel` / `redis-sentinel-ssl`；每个哨兵生成**独立** `sentinel.conf`（port / monitor / auth-pass / announce-ip），并提示 `sentinel myid` 需唯一（容器首次启动自动生成，勿复用他人 conf）
- **SSL 证书（双模式，已定 ✅）**：
  - **模式 A：应用内签发**（默认）：用户填必要信息（CN / SAN / 有效期 / **密钥类型下拉：RSA 2048 默认 / RSA 4096 / ECDSA P-256**），后端生成自建 CA + 服务器证书，输出 `ca.crt / ca.key / redis.crt / redis.key` 四段 PEM 到输出区（MeCode），复制或保存；可行性与依赖见 §五.11；对无 openssl 环境的用户（尤其 Windows 本地）更友好，与"仅 Linux 目标机"不冲突（签发在客户端，执行在服务器）
  - **模式 B：openssl 命令生成**（保留，供偏好手动操作的用户）：
  ```text
  openssl genrsa -out ca.key 4096
  openssl req -x509 -new -nodes -key ca.key -sha256 -days 3650 -out ca.crt -subj "/CN=redis-ca"
  openssl genrsa -out redis.key 2048
  openssl req -new -key redis.key -out redis.csr -subj "/CN=<announce-ip>"
  openssl x509 -req -in redis.csr -CA ca.crt -CAkey ca.key -CAcreateserial -days 3650 -sha256 -extfile <SAN 配置> -out redis.crt
  ```
  SAN 需包含 announce-ip 与 localhost（`subjectAltName=IP:<ip>,IP:127.0.0.1,DNS:localhost`）

### 3.4 输出示例（docker compose tab，集群片段，conf 挂载形态）

```yaml
services:
  redis8-7001:
    image: redis:8
    container_name: redis8-7001
    command: redis-server /etc/redis/redis.conf
    restart: always
    network_mode: 'host'
    environment:
      - TZ=Asia/Shanghai
    volumes:
      - ./conf/redis-7001.conf:/etc/redis/redis.conf
      - ./data-7001:/data
```

对应 `./conf/redis-7001.conf`：

```text
port 7001
requirepass <PASS>
masterauth <PASS>
cluster-enabled yes
appendonly yes
dir /data
# 如需禁用/改名命令等额外配置，在此自行追加，例如：
# rename-command FLUSHALL ""
```

---

## 四、交互流程

```text
选模式 → 填表单（IP/端口/密码/开关）→ 实时生成（表单变化即重渲染输出）
       → 切换 tab（命令 / compose / 分步指南）→ 复制或保存 → 用户去 Linux 执行
       → （可选）查看连接摘要，回连接页建连接
```

- 表单校验：端口范围、密码特殊字符转义提示、announce-ip 必填（集群/哨兵）
- 不做"执行"、不做"探测端口占用"（目标机不可达，探测无意义）

---

## 五、值得讨论的点（重点）

### 5.1 入口放哪里？（已定 ✅）

**主界面独立「工具箱」入口**，与连接管理解耦；不在新建连接弹窗加引导。

### 5.2 announce-ip 引导（已定 ✅：表单校验 + 产物注释双管齐下）

- 集群/哨兵在 host 网络下必须填宿主机**对外可达** IP，填错则集群 create 成功但客户端连不上槽位重定向失败
- 表单：集群/哨兵模式下必填校验 + 输入框提示文案；产物（分步指南/命令）头部注释附验证命令（`hostname -I` / `curl ifconfig.me`）与坑点说明；不做自动探测

### 5.3 一键脚本还是分步指南？（已定 ✅：分步指南）

**不做**单文件 `install.sh` 一键脚本。改为第三个 tab「**分步指南**」：

- 把「创建目录 → 写 compose/conf 文件 → openssl 生成证书 → docker compose up -d → cluster create」等步骤**合并在一个 MeCode 代码块**中展示（shell 高亮）
- 每个步骤前用注释清晰标注（如 `# ===== 步骤 3/5：启动容器 =====`），并注明执行前提（如"等待全部容器就绪后再执行下一步"）
- 用户自行逐步选中复制执行，出错时易定位，比黑盒一键脚本更可控

### 5.4 多文件产物展示（已定 ✅：主推分步指南，下拉选文件为补充）

- 哨兵模式产物 = 1 个 compose + N 个 sentinel.conf（+ 证书命令）
- 主交付走分步指南 heredoc（§五.13），多文件问题基本消化；compose/命令 tab 内保留二级下拉选文件查看逐个复制作为补充；不做 `.tar`/目录保存

### 5.5 网络模式（已定 ✅：默认 host）

- 默认 `network_mode: host`：Linux 专属、免端口映射、集群总线端口天然可达，与"仅 Linux"定位吻合（现有样例也全部如此）
- bridge 保留为高级选项；对集群很不友好（需手工映射 port 与 port+10000 总线端口、容器间网络互通问题），bridge + 集群组合给出强警告或直接禁用（实施时定）

### 5.6 版本选择（已定 ✅：下拉 + 自定义，不做能力联动）

- 下拉提供 `redis:8`（默认）/ `redis:7` / `valkey:8` + 自定义镜像名输入；不做版本能力联动校验（避免过度设计）
- 产物注释中提示：`redis:8` 滚动标签版本不固定，如需 Vector Set（≥ 8.4）请自行确认实际版本

### 5.7 密码特殊字符与转义（已定 ✅：统一转义策略）

- 策略钉死：
  - **redis.conf / sentinel.conf**：`requirepass "<密码>"` 双引号包裹，转义 `\` 与 `"`
  - **shell 命令**（redis-cli -a 等）：单引号包裹；密码含单引号时退回双引号 + `\$`/反斜杠转义，并在产物头部注释提醒
  - **yaml command 行**：随 conf 挂载形态基本消失（仅命令行形态保留时同样双引号包裹）
  - **heredoc**：带引号定界符 `<<'EOF'`，内容零转义（天然安全）
- 表单侧：密码输入框无限制，但生成时检测到含 `'`/`"`/`$`/空格等字符时在产物头部追加醒目注释提醒人工复核；实现上把转义逻辑收敛到生成器单一函数并配单测

### 5.8 与连接管理的联动（已定 ✅：首版纯展示摘要）

- 生成完成后展示「连接摘要」卡片（host/端口/密码/SSL 开关，集群展示任一节点 + 提示集群连接方式）；"预填新建连接"作 P2

### 5.9 持久化默认值（已定 ✅：默认开启）

- 数据持久化**默认开启**：`appendonly yes` + `./data-<port>:/data` volume，重启不丢数据更稳妥；表单保留开关可手动关闭（测试要净环境时）
- 哨兵配置目录是**另一回事**：哨兵必须挂载独立配置目录（§六.3），不受数据持久化开关控制

### 5.10 是否在产物中内置 redis.conf 调优项？

- 如 `maxmemory`、`maxmemory-policy`、`databases`（集群必须 1 库）等
- 倾向：首版不做内置调优项，只留"高级参数"透传输入框（conf 挂载形态下追加到 `redis.conf` 尾部，见 §五.16；命令行形态下追加到 `redis-server` 参数），避免膨胀

### 5.11 证书应用内签发（已定 ✅：做，与 openssl 命令生成并存）

依赖可行性结论（已核对 `Cargo.toml` / `Cargo.lock`）：

- 现有依赖 `rustls`（ring）、`russh`（rsa）已具备密钥运算基础，但**缺 X.509/ASN.1 编码能力**，需新增 **`rcgen`**（轻量、基于 ring，与现有依赖树无冲突，是当前 Rust 生态自签证书的标准选择）
- **密钥类型用户可选**（已定 ✅）：**RSA 2048（默认）/ RSA 4096 / ECDSA P-256**。Redis TLS 基于 OpenSSL，RSA 与 ECDSA 都支持；官方 TLS 文档示例用的就是 `genrsa 2048`，用户习惯也以 RSA 为主，故默认 RSA。RSA 生成不走 ring（ring 不支持 RSA 密钥*生成*），走 **rsa crate**——已随 `russh` 在依赖树中，`rcgen` 开 `rsa`/`pem` feature 即可，增量成本很小
- 用户填写项：CN、SAN（自动带 announce-ip + `127.0.0.1` + `localhost`，可增删）、有效期天数（默认 3650）；输出四段 PEM + 落盘路径建议（`./certs/`）
- CA + 服务器证书两级结构（与模式 B 的 openssl 命令对齐，便于后续用同一 CA 续期/加签新节点证书）
- 私钥以明文文本展示/保存：本地工具、用户主动行为，可接受；应用不做磁盘留存
- **证书文件如何送达服务器**（需讨论）：签发在客户端、部署在服务器，四个文件需落地目标机。倾向：分步指南中直接生成 `cat > ./certs/redis.crt <<'EOF' ... EOF` 的写文件命令，用户粘贴执行即落地；"保存文件后自行上传"作为备选（见 §五.13）
- **SSL 集群的组网命令**：`redis-cli --cluster create` 在 TLS 下需追加 `--tls --cert ./certs/redis.crt --key ./certs/redis.key --cacert ./certs/ca.crt`（redis-cli 自身也是 TLS 客户端），生成器不能忘拼这组参数；哨兵同理对齐现有 `redis-sentinel-ssl` 样例（`port 0` + `tls-port` + `tls-replication yes`，monitor 指向主节点 tls 端口）

---

### 5.12 单机/多机部署与多 IP 输入（已定 ✅：支持多机，单框分号分隔）

- **输入交互（已定）**：不做节点列表式表单；announce-ip 用**单个输入框，分号 `;` 分隔多个 IP**，自动分割、去空格、去重、逐个格式校验（IPv4/主机名，兼容全角 `；`）。1 个 IP = 单机模式（行为等同现有样例），N 个 IP = 多机模式（N 台宿主机）
- **节点分配规则**（多机核心设计）：
  - 集群：节点 i（0 起）→ `IP[i % N]`，端口 = 基准端口 + i（全局递增，同机天然不冲突）；`--cluster create` 命令按 ip:port 列表顺序拼，`--cluster-replicas 1` 由 redis-cli 自动错开主从机器（IP 数 ≥ 2 时天然跨机）
  - 哨兵：主节点 → IP[0]；从节点按剩余 IP 循环分配；哨兵每机一个循环分布（哨兵端口 = 哨兵基准 + idx）；`sentinel monitor` 指向主节点 ip:port
- **产物按机器分组**：多机时 compose 每台一份（命名 `docker-compose-<ip>.yaml`），分步指南按机器分段标注（`# ===== 机器 192.168.1.10 上执行 =====`）；与 §五.13 heredoc 交付方案合流，多文件展示问题一并消化（§5.4 降级）
- **校验与提示**：IP 数不整除节点数不硬拦（循环分配即可）；但提示「主从可能同机，跨机容灾能力有限」（如 6 节点 1 个 IP）；证书 SAN 自动收集全部 IP 入列（应用内签发时）
- 端口方案已定：**全局递增**（节点 i → 基准+i，实现简单、同机不冲突）

### 5.13 证书/配置文件的交付方式（已定 ✅：heredoc 内联）

- 问题：哨兵多份 `sentinel.conf`、每节点 `redis.conf`、应用内签发的四段 PEM，都是"多文件产物"，而交付目标是"复制即用"
- 已定：分步指南中用 `cat > 路径 <<'EOF'` heredoc 把每个文件内容内联成命令，粘贴执行即落地（与"逐步复制执行"交互一致）；输出区二级下拉选文件逐个复制作补充（§5.4）
- PEM 内容含换行与边界行（`-----BEGIN...`），heredoc 用带引号的定界符 `<<'EOF'` 防 shell 转义；需单测覆盖边界行冲突（内容中恰好出现 EOF 时换定界符）
- 与 §5.4 的关系：heredoc 落地后，5.4 的多文件展示问题基本被分步指南消化，docker compose tab 的多文件展示（含多机按机器分 compose，见 §五.12）降级为 P2

### 5.14 分步指南的复制交互（已定 ✅：逐步复制按钮 + 整块复制）

- 每个步骤注释块旁配**独立复制按钮**（按步骤切分文本），同时保留整块复制；步骤间注释标注前置条件（如"等全部容器就绪：`docker compose ps` 确认"）
- 实现：生成器返回结构化步骤数组 `{ title, code }`，分步指南 tab 渲染时拼成整块展示，复制按步骤取文本（命令/compose tab 为单块退化形态）

### 5.15 镜像变体（已定 ✅：提供 alpine 开关）

- 提供 `alpine` 开关（如 `redis:8-alpine`，体积小），成本极低；注意 alpine 镜像无 bash（分步指南命令用 POSIX sh）
- `redis:8` 滚动标签版本不固定：连接摘要/产物注释中提示"如需 Vector Set（≥ 8.4）请确认实际版本 `docker run --rm redis:8 redis-server --version`"

### 5.16 配置文件外置挂载（已定 ✅：默认开启，与数据一致）

- 动机：命令行参数形态在集群+SSL 下可达十余个参数，又长又难改；conf 挂载后启动命令仅 `redis-server /etc/redis/redis.conf` 一行；用户编辑 conf 即可追加 `rename-command`/禁用命令、maxmemory 等定制配置，无需懂 compose 与重启命令拼接
- 形态：每节点一份 `./conf/redis-<port>.conf`（多机时按机器分组，§五.12）；哨兵维持 `./conf/sentinel-<port>/sentinel.conf`（有状态，§六.3）
- conf 生成内容：port / tls-*、requirepass / masterauth、cluster-enabled / replicaof、cluster-announce-ip / replica-announce-ip、appendonly、`dir /data`（保证 RDB / AOF / 集群 `nodes.conf` 落挂载目录，容器重建不丢）；末尾附注释掉的 `rename-command` 示例引导用户定制；高级参数透传输入框（§5.10）直接追加到 conf 尾部
- **docker 命令 tab 例外**：保留命令行参数形态（一条 `docker run` 即跑，不依赖先建文件）；compose / 分步指南 tab 默认 conf 挂载；两种形态差异在 UI 文案说明
- 与 §5.13 的关系：conf 文件数随节点数增长（每节点一份），heredoc 内联写文件成为主要交付方式

---

## 六、技术注意事项

1. **集群总线端口**：bus = port+10000，host 网络下需用户防火墙放行；生成产物头部注释必须提醒
2. **`--cluster create` 执行时机**：容器全部起来后执行；脚本形态需加 `sleep` / `docker compose up -d && sleep 3` 之类的保护，否则节点未就绪会失败
3. **sentinel.conf 是有状态文件**：哨兵运行时会自动重写（写入 myid、已知副本），**必须挂载独立目录**，不能用 `--sentinel` 命令行参数替代；同一套 conf 不能被多个哨兵复用（myid 冲突）
4. **compose 命令差异**：输出统一用 `docker compose`（v2 插件）；注释中提示旧环境 `docker-compose` 等价
5. **密码/证书路径**：现有样例证书放 `../redis.crt` 相对路径，生成器统一规范化为 `./certs/` 子目录，避免用户照搬出歧义
6. **后端改动仅一处**：除证书应用内签发的 Tauri command（§五.11）外，其余均为纯前端文本模板拼接；模板放 `src/utils/`（如 `redis-install-gen.ts`）
7. **CodeMirror 语言**：shell / yaml 双语言高亮切换（检查现有 `plugins/codemirror.ts` 已加载的 language 包，缺则补）
8. **换行符**：产物面向 Linux，导出文件时必须 **LF**（对齐仓库换行符规范）
9. **数据目录权限坑**：官方 `redis` 镜像以 `redis` 用户（uid 999）运行，宿主机新建的 `./data-*` 目录 bind mount 后容器内可能无写权限，导致 RDB/AOF 落盘失败；分步指南需包含 `sudo chown -R 999:999 ./data-*`（或 chmod）步骤
10. **持久化与集群**：集群每节点独立数据目录（`./data-<port>`）；`appendonly yes` 下首次启动会生成 AOF 目录（8.0 为 multi-part AOF），目录结构勿手工乱动

---

## 七、改动文件清单（预期）

### 后端

1. `Cargo.toml`：新增 `rcgen`（自签证书生成，开 `pem`/`rsa` feature；RSA 走已在依赖树的 rsa crate，ECDSA 走已有 ring）
2. `src-tauri/src/api.rs`：新增 `tls_gen_cert` Tauri command（入参：CN / SAN 列表 / 有效期天数 / **密钥类型（RSA2048/RSA4096/ECDSA-P256）**；返回 `ca.crt / ca.key / redis.crt / redis.key` 四段 PEM），Specta 类型同步导出

### 前端

1. `src/utils/redis-install-gen.ts`：生成器核心（模式 × 选项 → 文本产物），纯函数可单测
2. `src/views/ext/RedisInstall.vue`（或同级工具页）：表单 + MeCode 输出 + tab 切换
3. 入口注册（主界面工具区）+ 路由
4. `locales/{zh,en}`：表单与提示文案
5. `plugins/codemirror.ts`：如缺 shell/yaml 语言包则补

### 测试

- 生成器纯函数单测（`vp test`）：覆盖三种模式 × SSL/持久化开关 × 密码转义边界
- 手工验收：把产物复制到 Linux 真机跑通（对照 `01_redis-install` 现有样例验证等价性，再验证 `--cluster create` 一步成团）

---

## 八、有意不做（首版）

- 应用内执行 docker 命令 / 远程 SSH 部署
- 单文件一键 `install.sh`（已改用分步指南，见 §五.3）
- Windows / macOS 目标机
- K8s / Helm / systemd / 裸机编译
- redis.conf 调优向导（仅高级参数透传）
- 已有集群扩容 / 缩容 / 迁移脚本生成
