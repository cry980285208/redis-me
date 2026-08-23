// Redis 安装帮助：Docker 部署产物生成器（纯函数，无 I/O）
// 三种模式（单机/集群/哨兵）× 三种产物（docker 命令 / docker compose / 分步指南），目标机仅考虑 Linux

export type RedisInstallMode = 'single' | 'cluster' | 'sentinel'
export type RedisInstallLang = 'shell' | 'yaml' | 'conf'

// 产物中的一个步骤块（界面按块展示 + 逐块复制）
export interface RedisInstallStep {
  title: string
  code: string
  lang: RedisInstallLang
}

export interface RedisInstallOptions {
  mode: RedisInstallMode
  image: string
  alpine: boolean
  // 节点机器 IP 列表（前端分号分隔拆分）；空则按 ['127.0.0.1']
  ips: string[]
  basePort: number
  password: string
  // cluster
  clusterMasters: number
  clusterReplicasPerMaster: number
  // sentinel（一主 N 从 + 哨兵）
  sentinelReplicas: number
  sentinelCount: number
  mountData: boolean
  mountConf: boolean
  ssl: boolean
  timezone: string
  // 附加 docker run 参数（仅作用于 docker 命令形态）
  extraArgs: string
}

export interface RedisInstallNode {
  name: string
  ip: string
  port: number
  role: 'master' | 'replica' | 'sentinel'
}

// 步骤标题文案由视图层传入（i18n），生成器保持语言无关
export interface RedisInstallLabels {
  machine: string
  stepEnv: string
  stepConf: string
  stepCert: string
  stepStart: string
  stepCluster: string
  stepVerify: string
  composeFile: string
}

// 证书生成脚本注释文案（i18n）
export interface RedisCertLabels {
  scriptTitle: string
  scriptOutput: string
  step1Title: string
  step1Desc: string
  step2Title: string
  step2Desc: string
  step2Note: string
  step3Title: string
  step4Title: string
  step5Title: string
  step6Title: string
}

export interface RedisInstallOutput {
  nodes: RedisInstallNode[]
  commands: RedisInstallStep[]
  compose: RedisInstallStep[]
  guide: RedisInstallStep[]
}

// 宿主机目录约定：/data 下按模式建安装目录，节点位于 <root>/<容器名>/{data,*.conf}，证书位于 <root>/cert
function hostRoot(mode: RedisInstallMode): string {
  return `/data/redis-${mode}`
}
function certDir(mode: RedisInstallMode): string {
  return `${hostRoot(mode)}/cert`
}
// 官方镜像以 redis 用户（uid 999）运行，数据/证书目录需授权
const REDIS_UID = '999:999'

// 各模式默认起始端口：集群惯例 7001 段（7001~7006）；单机/哨兵用经典 6379 段（哨兵组 +20000）
export function genInstallDefaultPort(mode: RedisInstallMode): number {
  return mode === 'cluster' ? 7001 : 6379
}

// 网络模式（内置约定）：单机用简单端口映射；集群/哨兵多端口 + 总线/通告需求，用宿主机网络更简单可靠
function hostNetwork(mode: RedisInstallMode): boolean {
  return mode !== 'single'
}

// ---------------- 转义工具 ----------------

// redis.conf 值引用：双引号包裹并转义 \ 与 "（简单 token 可裸写）
function confQuote(v: string): string {
  if (/^[A-Za-z0-9._:/@=-]+$/.test(v)) return v
  return `"${v.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`
}

// shell 参数引用：单引号优先；含单引号时退化为双引号转义
function shellQuote(v: string): string {
  if (/^[A-Za-z0-9._:/@=-]+$/.test(v)) return v
  if (!v.includes("'")) return `'${v}'`
  return `"${v.replace(/\\/g, '\\\\').replace(/"/g, '\\"').replace(/\$/g, '\\$').replace(/`/g, '\\`')}"`
}

// heredoc 写文件：定界符带引号，内容零转义；内容与定界符冲突时自动换名
function heredoc(path: string, content: string): string {
  const body = content.endsWith('\n') ? content : content + '\n'
  let delim = 'EOF'
  let i = 1
  while (body.includes(delim)) delim = `EOF${i++}`
  return `cat > ${path} <<'${delim}'\n${body}${delim}`
}

// ---------------- 节点分配 ----------------

export function genInstallNodes(o: RedisInstallOptions): RedisInstallNode[] {
  const ips = o.ips.filter(ip => ip.trim())
  const hosts = ips.length > 0 ? ips : ['127.0.0.1']
  const pick = (i: number) => hosts[i % hosts.length]
  const nodes: RedisInstallNode[] = []
  // 容器/目录命名：redis-<端口>（端口在部署内唯一）；单机直接落在模式目录，不再多一层节点目录
  if (o.mode === 'single') {
    nodes.push({ name: `redis-${o.basePort}`, ip: hosts[0], port: o.basePort, role: 'master' })
  } else if (o.mode === 'cluster') {
    // 端口全局递增；redis-cli cluster create 将前 N 个节点作为主节点
    const total = o.clusterMasters * (1 + o.clusterReplicasPerMaster)
    for (let i = 0; i < total; i++) {
      const port = o.basePort + i
      nodes.push({
        name: `redis-${port}`,
        ip: pick(i),
        port,
        role: i < o.clusterMasters ? 'master' : 'replica',
      })
    }
  } else {
    // 哨兵：一主 + N 从 + 哨兵组（哨兵端口段 base+20000 起，避开集群总线端口段 +10000）
    nodes.push({ name: `redis-${o.basePort}`, ip: hosts[0], port: o.basePort, role: 'master' })
    for (let r = 0; r < o.sentinelReplicas; r++) {
      nodes.push({
        name: `redis-${o.basePort + 1 + r}`,
        ip: pick(r + 1),
        port: o.basePort + 1 + r,
        role: 'replica',
      })
    }
    for (let s = 0; s < o.sentinelCount; s++) {
      nodes.push({
        name: `redis-${o.basePort + 20000 + s}`,
        ip: pick(s),
        port: o.basePort + 20000 + s,
        role: 'sentinel',
      })
    }
  }
  return nodes
}

// 证书 SAN：全部节点 IP + 本机回环
export function genInstallSans(o: RedisInstallOptions): string[] {
  const sans = new Set<string>()
  for (const n of genInstallNodes(o)) sans.add(n.ip)
  sans.add('127.0.0.1')
  sans.add('localhost')
  return [...sans]
}

function finalImage(o: RedisInstallOptions): string {
  return o.alpine && !o.image.includes('alpine') ? `${o.image}-alpine` : o.image
}

// 哨兵必须挂载配置文件（monitor 等指令无法用命令行参数表达）
function confMounted(o: RedisInstallOptions): boolean {
  return o.mountConf || o.mode === 'sentinel'
}

// 节点宿主机目录：单机直接落在模式目录，集群/哨兵每节点一层（以容器名命名）
function nodeDir(o: RedisInstallOptions, node: RedisInstallNode): string {
  const root = hostRoot(o.mode)
  return o.mode === 'single' ? root : `${root}/${node.name}`
}

// 端口配置行（SSL 时关闭明文端口，启用 TLS 端口）
function portLines(ssl: boolean, port: number): string[] {
  return ssl ? ['port 0', `tls-port ${port}`] : [`port ${port}`]
}

// TLS 证书与协议公共配置行
function tlsBaseLines(): string[] {
  return [
    'tls-cert-file /etc/redis/redis.crt',
    'tls-key-file /etc/redis/redis.key',
    'tls-ca-cert-file /etc/redis/ca.crt',
    'tls-protocols "TLSv1.2 TLSv1.3"',
  ]
}

// 按机器分组（保持首次出现顺序）
function groupByMachine(nodes: RedisInstallNode[]): [string, RedisInstallNode[]][] {
  const map = new Map<string, RedisInstallNode[]>()
  for (const n of nodes) {
    if (!map.has(n.ip)) map.set(n.ip, [])
    map.get(n.ip)!.push(n)
  }
  return [...map.entries()]
}

// ---------------- 配置文件内容 ----------------

function redisConf(o: RedisInstallOptions, node: RedisInstallNode): string {
  const l: string[] = []
  l.push(...portLines(o.ssl, node.port))
  l.push('bind 0.0.0.0', 'protected-mode no')
  if (o.password) l.push(`requirepass ${confQuote(o.password)}`)
  if (o.mode === 'cluster') {
    l.push('cluster-enabled yes')
    if (o.password) l.push(`masterauth ${confQuote(o.password)}`)
    // host 网络多网卡时保证节点间通告地址正确
    l.push(
      `cluster-announce-ip ${node.ip}`,
      `cluster-announce-port ${node.port}`,
      `cluster-announce-bus-port ${node.port + 10000}`,
    )
  } else if (o.mode === 'sentinel' && node.role === 'replica') {
    if (o.password) l.push(`masterauth ${confQuote(o.password)}`)
    l.push(`replica-announce-ip ${node.ip}`, `replica-announce-port ${node.port}`)
  }
  l.push('dir /data')
  if (o.ssl) {
    l.push(...tlsBaseLines())
    if (o.mode !== 'single') l.push('tls-replication yes')
    if (o.mode === 'cluster') l.push('tls-cluster yes')
  }
  return l.join('\n') + '\n'
}

function sentinelConf(
  o: RedisInstallOptions,
  node: RedisInstallNode,
  master: RedisInstallNode,
): string {
  const quorum = Math.floor(o.sentinelCount / 2) + 1
  const l: string[] = []
  l.push(...portLines(o.ssl, node.port))
  if (o.password) l.push(`requirepass ${confQuote(o.password)}`)
  l.push(
    `sentinel monitor mymaster ${master.ip} ${master.port} ${quorum}`,
    `sentinel down-after-milliseconds mymaster 5000`,
    `sentinel failover-timeout mymaster 60000`,
  )
  if (o.password) l.push(`sentinel auth-pass mymaster ${confQuote(o.password)}`)
  l.push(`sentinel announce-ip ${node.ip}`, `sentinel announce-port ${node.port}`, 'dir /data')
  if (o.ssl) {
    l.push(...tlsBaseLines(), 'tls-replication yes')
  }
  return l.join('\n') + '\n'
}

// 不挂载配置文件时的命令行参数形态（哨兵不适用）
// Docker 官方镜像 entrypoint 已默认 --bind 0.0.0.0 --protected-mode no --dir /data，此处仅补充差异参数
function confArgs(o: RedisInstallOptions, node: RedisInstallNode): string[] {
  const args: string[] = []
  if (o.ssl) {
    args.push('--port', '0', '--tls-port', `${node.port}`)
  } else if (node.port !== 6379) {
    args.push('--port', `${node.port}`)
  }
  if (o.password) args.push('--requirepass', o.password)
  if (o.mode === 'cluster') {
    args.push('--cluster-enabled', 'yes')
    if (o.password) args.push('--masterauth', o.password)
    args.push(
      '--cluster-announce-ip',
      node.ip,
      '--cluster-announce-port',
      `${node.port}`,
      '--cluster-announce-bus-port',
      `${node.port + 10000}`,
    )
  }
  if (o.ssl) {
    args.push(
      '--tls-cert-file',
      '/etc/redis/redis.crt',
      '--tls-key-file',
      '/etc/redis/redis.key',
      '--tls-ca-cert-file',
      '/etc/redis/ca.crt',
    )
    if (o.mode === 'cluster') args.push('--tls-replication', 'yes', '--tls-cluster', 'yes')
  }
  return args
}

// ---------------- docker run 形态 ----------------

function dockerRunCmd(o: RedisInstallOptions, node: RedisInstallNode): string {
  const dir = nodeDir(o, node)
  const parts = [`docker run -d --name ${node.name} --restart unless-stopped`]
  if (hostNetwork(o.mode)) parts.push('--network host')
  else parts.push(`-p ${node.port}:${node.port}`)
  if (o.timezone) parts.push(`-e TZ=${shellQuote(o.timezone)}`)
  if (o.mountData) parts.push(`-v ${dir}/data:/data`)
  if (node.role === 'sentinel') {
    parts.push(`-v ${dir}/sentinel.conf:/etc/redis/sentinel.conf`)
  } else if (confMounted(o)) {
    parts.push(`-v ${dir}/redis.conf:/etc/redis/redis.conf`)
  }
  if (o.ssl) parts.push(`-v ${certDir(o.mode)}:/etc/redis:ro`)
  if (o.extraArgs.trim()) parts.push(o.extraArgs.trim())
  parts.push(finalImage(o))
  if (node.role === 'sentinel') {
    parts.push('redis-server /etc/redis/sentinel.conf --sentinel')
  } else if (confMounted(o)) {
    parts.push('redis-server /etc/redis/redis.conf')
  } else {
    parts.push(['redis-server', ...confArgs(o, node).map(shellQuote)].join(' '))
  }
  return parts.join(' \\\n  ')
}

// redis-cli 公共参数（认证 + TLS）
function redisCliPrefix(o: RedisInstallOptions): string {
  let cli = 'redis-cli'
  const dir = certDir(o.mode)
  if (o.ssl) cli += ` --tls --cert ${dir}/redis.crt --key ${dir}/redis.key --cacert ${dir}/ca.crt`
  return cli
}

function redisCliAuth(o: RedisInstallOptions): string {
  return o.password ? ` -a ${shellQuote(o.password)} --no-auth-warning` : ''
}

// ---------------- 证书步骤 ----------------

const CERT_SEP = '# ----------------------------------------------'

// openssl 自签证书脚本（固定 RSA 4096；每步带注释说明）
// 证书弹框与安装指南共用此函数
export function genOpensslCertScript(p: {
  sans: string[]
  certDays: number
  certCn: string
  labels: RedisCertLabels
}): string {
  const cn = p.certCn.trim() || 'redis'
  const sans = p.sans.map(s => (/^\d+$|[:.]/.test(s) ? `IP:${s}` : `DNS:${s}`)).join(',')
  const sep = CERT_SEP
  const L = p.labels
  return [
    '# ============================================',
    `# ${L.scriptTitle}`,
    `# ${L.scriptOutput}`,
    '# ============================================',
    '',
    sep,
    `# ${L.step1Title}`,
    `# ${L.step1Desc}`,
    sep,
    'openssl genrsa -out ca.key 4096',
    '',
    sep,
    `# ${L.step2Title}`,
    `# ${L.step2Desc}`,
    `# ${L.step2Note}`,
    sep,
    heredoc(
      'openssl.cnf',
      `[req]
prompt = no
distinguished_name = req_distinguished_name

[req_distinguished_name]
C = CN
O = Redis
CN = ${cn}

[v3_ca]
basicConstraints = critical, CA:true
keyUsage = critical, keyCertSign, cRLSign
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid:always,issuer

[v3_req]
subjectAltName = ${sans}
`,
    ),
    `openssl req -x509 -new -nodes -sha256 -key ca.key -days ${p.certDays} -config openssl.cnf -extensions v3_ca -out ca.crt`,
    '',
    sep,
    `# ${L.step3Title}`,
    sep,
    'openssl genrsa -out redis.key 4096',
    '',
    sep,
    `# ${L.step4Title}`,
    sep,
    `openssl req -new -sha256 -key redis.key -config openssl.cnf -out redis.csr`,
    '',
    sep,
    `# ${L.step5Title}`,
    sep,
    `openssl x509 -req -sha256 -in redis.csr -CA ca.crt -CAkey ca.key -CAcreateserial -days ${p.certDays} -extfile openssl.cnf -extensions v3_req -out redis.crt`,
    '',
    sep,
    `# ${L.step6Title}`,
    sep,
    "openssl x509 -in redis.crt -text -noout | grep -E 'Version|Not|Subject:|Alternative|IP Address|DNS'",
  ].join('\n')
}

function certStepOpenssl(
  o: RedisInstallOptions,
  labels: RedisInstallLabels,
  multi: boolean,
): RedisInstallStep {
  const dir = certDir(o.mode)
  const parts: string[] = [
    '# 将生成的 ca.crt、redis.crt、redis.key 复制到以下目录',
    `mkdir -p ${dir}`,
  ]
  if (multi) parts.push(`# 多机部署：将证书文件分发到每台机器的 ${dir} 目录`)
  parts.push('', `chown -R ${REDIS_UID} ${hostRoot(o.mode)}`)
  return { title: labels.stepCert, code: parts.join('\n'), lang: 'shell' }
}

// ---------------- 主入口 ----------------

export function genRedisInstall(
  o: RedisInstallOptions,
  labels: RedisInstallLabels,
): RedisInstallOutput {
  const nodes = genInstallNodes(o)
  const machines = groupByMachine(nodes)
  const multi = machines.length > 1
  const machineTitle = (base: string, ip: string) =>
    multi ? `${base} - ${labels.machine} ${ip}` : base
  const master = nodes.find(n => n.role === 'master')!
  const sentinelNodes = nodes.filter(n => n.role === 'sentinel')
  const root = hostRoot(o.mode)
  const certs = certDir(o.mode)

  // ===== docker 命令形态 =====
  const commands: RedisInstallStep[] = []
  for (const [ip, ns] of machines) {
    const blocks: string[] = []
    for (const n of ns) {
      // 目录创建：confMounted 需节点目录，mountData 需 data 子目录（mkdir -p 幂等，同时覆盖父目录）
      if (confMounted(o)) blocks.push(`mkdir -p ${nodeDir(o, n)}`)
      if (o.mountData) blocks.push(`mkdir -p ${nodeDir(o, n)}/data`)
      if (n.role !== 'sentinel' && confMounted(o)) {
        blocks.push(heredoc(`${nodeDir(o, n)}/redis.conf`, redisConf(o, n)))
      }
      if (n.role === 'sentinel') {
        blocks.push(heredoc(`${nodeDir(o, n)}/sentinel.conf`, sentinelConf(o, n, master)))
      }
      blocks.push(dockerRunCmd(o, n))
    }
    if (o.mountData) blocks.push(`chown -R ${REDIS_UID} ${root}`)
    commands.push({
      title: machineTitle('docker run', ip),
      code: blocks.join('\n\n'),
      lang: 'shell',
    })
  }

  // ===== docker compose 形态（按机器一个文件）=====
  const compose: RedisInstallStep[] = []
  for (const [ip, ns] of machines) {
    const lines: string[] = ['services:']
    for (const n of ns) {
      lines.push(`  ${n.name}:`)
      lines.push(`    image: ${finalImage(o)}`)
      lines.push(`    container_name: ${n.name}`)
      if (hostNetwork(o.mode)) {
        lines.push('    network_mode: host')
      } else {
        lines.push('    ports:')
        lines.push(`      - '${n.port}:${n.port}'`)
      }
      lines.push('    restart: unless-stopped')
      if (o.timezone) {
        lines.push('    environment:')
        lines.push(`      - TZ=${o.timezone}`)
      }
      const vols: string[] = []
      if (o.mountData) vols.push(`      - ${nodeDir(o, n)}/data:/data`)
      if (n.role === 'sentinel')
        vols.push(`      - ${nodeDir(o, n)}/sentinel.conf:/etc/redis/sentinel.conf`)
      else if (confMounted(o))
        vols.push(`      - ${nodeDir(o, n)}/redis.conf:/etc/redis/redis.conf`)
      if (o.ssl) vols.push(`      - ${certs}:/etc/redis:ro`)
      if (vols.length > 0) {
        lines.push('    volumes:')
        lines.push(...vols)
      }
      if (n.role === 'sentinel')
        lines.push('    command: redis-server /etc/redis/sentinel.conf --sentinel')
      else if (confMounted(o)) lines.push('    command: redis-server /etc/redis/redis.conf')
      else
        lines.push(
          `    command: redis-server ${confArgs(o, n)
            .map(a => (shellQuote(a) === a ? a : JSON.stringify(a)))
            .join(' ')}`,
        )
    }
    compose.push({
      title: `${labels.composeFile} (${ip})`,
      code: lines.join('\n') + '\n',
      lang: 'yaml',
    })
  }

  // ===== 分步指南 =====
  const guide: RedisInstallStep[] = []
  // 1. 环境准备
  for (const [ip, ns] of machines) {
    const dirs: string[] = []
    for (const n of ns) {
      if (confMounted(o)) dirs.push(nodeDir(o, n))
      if (o.mountData) dirs.push(`${nodeDir(o, n)}/data`)
    }
    if (o.ssl) dirs.push(certs)
    guide.push({
      title: machineTitle(labels.stepEnv, ip),
      code: `mkdir -p ${dirs.join(' ')}`,
      lang: 'shell',
    })
  }
  // 2. 写入配置文件
  for (const [ip, ns] of machines) {
    const blocks: string[] = []
    for (const n of ns) {
      if (n.role === 'sentinel')
        blocks.push(heredoc(`${nodeDir(o, n)}/sentinel.conf`, sentinelConf(o, n, master)))
      else if (confMounted(o)) blocks.push(heredoc(`${nodeDir(o, n)}/redis.conf`, redisConf(o, n)))
    }
    // 官方镜像以 redis 用户运行，数据/证书目录需授权（配置文件仅读取，无需授权）
    if (!o.ssl) blocks.push(`chown -R ${REDIS_UID} ${root}`)
    if (blocks.length > 0) {
      guide.push({
        title: machineTitle(labels.stepConf, ip),
        code: blocks.join('\n\n'),
        lang: 'shell',
      })
    }
  }
  // 3. 证书
  if (o.ssl) {
    guide.push(certStepOpenssl(o, labels, multi))
  }
  // 4. 启动容器
  for (const [ip, ns] of machines) {
    guide.push({
      title: machineTitle(labels.stepStart, ip),
      code: ns.map(n => dockerRunCmd(o, n)).join('\n\n'),
      lang: 'shell',
    })
  }
  // 5. 集群初始化 / 哨兵验证
  const cli = redisCliPrefix(o)
  const auth = redisCliAuth(o)
  if (o.mode === 'cluster') {
    const blocks: string[] = ['# Wait for all nodes to be ready']
    for (const n of nodes) blocks.push(`${cli} -h ${n.ip} -p ${n.port}${auth} ping`)
    blocks.push('')
    blocks.push('# Initialize cluster')
    blocks.push(
      `${cli}${auth} --cluster create ${nodes.map(n => `${n.ip}:${n.port}`).join(' ')} --cluster-replicas ${o.clusterReplicasPerMaster} --cluster-yes`,
    )
    guide.push({ title: labels.stepCluster, code: blocks.join('\n'), lang: 'shell' })
    guide.push({
      title: labels.stepVerify,
      code: [
        `${cli} -h ${master.ip} -p ${master.port}${auth} cluster info`,
        `${cli} -h ${master.ip} -p ${master.port}${auth} cluster nodes`,
      ].join('\n'),
      lang: 'shell',
    })
  } else if (o.mode === 'sentinel') {
    const s0 = sentinelNodes[0]
    guide.push({
      title: labels.stepVerify,
      code: [
        `${cli} -h ${master.ip} -p ${master.port}${auth} ping`,
        `${cli} -h ${master.ip} -p ${master.port}${auth} info replication`,
        `# Sentinel`,
        `${cli} -h ${s0.ip} -p ${s0.port}${auth} sentinel master mymaster`,
      ].join('\n'),
      lang: 'shell',
    })
  } else {
    guide.push({
      title: labels.stepVerify,
      code: `${cli} -h ${master.ip} -p ${master.port}${auth} ping`,
      lang: 'shell',
    })
  }

  return { nodes, commands, compose, guide }
}
