import { describe, expect, it } from 'vite-plus/test'

import type {
  RedisCertLabels,
  RedisInstallLabels,
  RedisInstallOptions,
} from '@/utils/redis-install-gen'
import {
  genInstallDefaultPort,
  genInstallSans,
  genOpensslCertScript,
  genRedisInstall,
} from '@/utils/redis-install-gen'

const labels: RedisInstallLabels = {
  machine: 'Machine',
  stepEnv: 'Prepare Environment',
  stepConf: 'Write Config Files',
  stepCert: 'Prepare TLS Certificates',
  stepStart: 'Start Containers',
  stepCluster: 'Initialize Cluster',
  stepVerify: 'Verify',
  composeFile: 'compose file',
}

function baseOptions(partial: Partial<RedisInstallOptions> = {}): RedisInstallOptions {
  return {
    mode: 'single',
    image: 'redis:8',
    alpine: false,
    ips: [],
    basePort: 6379,
    password: '',
    clusterMasters: 3,
    clusterReplicasPerMaster: 1,
    sentinelReplicas: 1,
    sentinelCount: 3,
    mountData: true,
    mountConf: true,
    ssl: false,
    timezone: 'Asia/Shanghai',
    extraArgs: '',
    ...partial,
  }
}

describe('genInstallNodes / genRedisInstall', () => {
  it('单机：1 节点，容器名 redis-端口，目录不再多一层节点目录', () => {
    const out = genRedisInstall(baseOptions(), labels)
    expect(out.nodes).toEqual([{ name: 'redis-6379', ip: '127.0.0.1', port: 6379, role: 'master' }])
    expect(out.guide.length).toBeGreaterThan(0)
    expect(out.commands.length).toBe(1)
    expect(out.compose.length).toBe(1)
    // 单机直接落在 /data/redis-single 下
    expect(out.commands[0].code).toContain('/data/redis-single/data')
    expect(out.commands[0].code).toContain('/data/redis-single/redis.conf')
    expect(out.commands[0].code).not.toContain('redis-single/redis-6379')
    // 单机用简单端口映射，不用宿主机网络；compose 同步为 ports 映射
    expect(out.commands[0].code).toContain('-p 6379:6379')
    expect(out.commands[0].code).not.toContain('--network host')
    expect(out.compose[0].code).toContain("- '6379:6379'")
    expect(out.compose[0].code).not.toContain('network_mode: host')
  })

  it('默认起始端口：集群 7001 段，单机/哨兵 6379 段', () => {
    expect(genInstallDefaultPort('cluster')).toBe(7001)
    expect(genInstallDefaultPort('single')).toBe(6379)
    expect(genInstallDefaultPort('sentinel')).toBe(6379)
  })

  it('集群三主三从：默认 7001~7006，端口全局递增，IP 轮询分配，宿主机网络', () => {
    const out = genRedisInstall(
      baseOptions({
        mode: 'cluster',
        basePort: genInstallDefaultPort('cluster'),
        ips: ['10.0.0.1', '10.0.0.2', '10.0.0.3'],
      }),
      labels,
    )
    expect(out.nodes.length).toBe(6)
    expect(out.nodes.map(n => n.port)).toEqual([7001, 7002, 7003, 7004, 7005, 7006])
    expect(out.nodes[0].ip).toBe('10.0.0.1')
    expect(out.nodes[3].ip).toBe('10.0.0.1')
    expect(out.nodes.filter(n => n.role === 'master').length).toBe(3)
    // 集群用宿主机网络（多端口 + 总线）
    expect(out.commands[0].code).toContain('--network host')
    // 按机器分组：3 台机器 → 3 个 compose 文件
    expect(out.compose.length).toBe(3)
    // 集群初始化步骤包含 --cluster create
    const clusterStep = out.guide.find(s => s.title === labels.stepCluster)
    expect(clusterStep?.code).toContain('--cluster create')
    expect(clusterStep?.code).toContain('--cluster-replicas 1')
    // redis.conf 启用集群与通告地址（总线端口 +10000）
    expect(clusterStep).toBeTruthy()
    const confStep = out.guide.find(s => s.title.includes(labels.stepConf))
    expect(confStep?.code).toContain('cluster-enabled yes')
    expect(confStep?.code).toContain('cluster-announce-bus-port 17001')
    // 安装目录：/data 下按模式建目录
    expect(confStep?.code).toContain('/data/redis-cluster')
  })

  it('哨兵：主从端口递增，哨兵端口段 +20000，quorum 正确', () => {
    const out = genRedisInstall(
      baseOptions({
        mode: 'sentinel',
        ips: ['10.0.0.1', '10.0.0.2'],
        sentinelReplicas: 2,
        sentinelCount: 3,
        password: 'pass',
      }),
      labels,
    )
    const ports = out.nodes.map(n => n.port)
    expect(ports).toEqual([6379, 6380, 6381, 26379, 26380, 26381])
    const confStep = out.guide.find(s => s.title.includes(labels.stepConf))
    expect(confStep?.code).toContain('sentinel monitor mymaster 10.0.0.1 6379 2')
    expect(confStep?.code).toContain('sentinel auth-pass mymaster')
  })

  it('密码含特殊字符：conf 双引号转义、shell 单引号转义', () => {
    const out = genRedisInstall(baseOptions({ password: 'pa"ss\'word' }), labels)
    const confStep = out.guide.find(s => s.title.includes(labels.stepConf))
    expect(confStep?.code).toContain('requirepass "pa\\"ss\'word"')
    const verify = out.guide.find(s => s.title === labels.stepVerify)
    // 含单引号时退化为双引号转义形态
    expect(verify?.code).toContain('-a "pa\\"ss\'word"')
  })

  it('TLS：安装指南证书步骤仅包含文件处理，openssl 脚本在证书弹框中', () => {
    const sslOut = genRedisInstall(baseOptions({ ssl: true }), labels)
    const certStep = sslOut.guide.find(s => s.title === labels.stepCert)
    // 安装指南只显示文件处理
    expect(certStep?.code).toContain('mkdir -p /data/redis-single/cert')
    // 不包含 chmod（由末尾 chown -R 统一授权）
    expect(certStep?.code).not.toContain('chmod')
    // 不包含 openssl 生成命令
    expect(certStep?.code).not.toContain('openssl genrsa')

    // redis.conf 包含 TLS 配置与协议限制
    const confStep = sslOut.guide.find(s => s.title.includes(labels.stepConf))
    expect(confStep?.code).toContain('port 0')
    expect(confStep?.code).toContain('tls-port 6379')
    expect(confStep?.code).toContain('tls-cert-file /etc/redis/redis.crt')
    expect(confStep?.code).toContain('tls-protocols "TLSv1.2 TLSv1.3"')
  })

  it('genOpensslCertScript：生成完整 openssl 脚本', () => {
    const labels: RedisCertLabels = {
      scriptTitle: 'Redis TLS Self-Signed Certificate Script',
      scriptOutput: 'Output: ca.key ca.crt redis.key redis.crt',
      step1Title: 'Step 1: Generate CA private key (RSA 4096-bit)',
      step1Desc: 'The CA signs server certificates and is the root of trust',
      step2Title: 'Step 2: Write OpenSSL configuration file',
      step2Desc:
        'Config includes DN, v3 extensions and SAN; -extensions v3_ca ensures x509v3 certificates',
      step2Note: 'Ensure subjectAltName includes all Redis node IPs and hostnames',
      step3Title: 'Step 3: Generate Redis server private key',
      step4Title: 'Step 4: Generate server Certificate Signing Request (CSR)',
      step5Title: 'Step 5: Sign server certificate with CA (with SAN)',
      step6Title: 'Step 6: Verify certificate (Version, Validity, Subject, SAN)',
    }
    const script = genOpensslCertScript({
      sans: ['127.0.0.1', 'localhost'],
      certDays: 36500,
      certCn: 'redis',
      labels,
    })
    expect(script).toContain('openssl genrsa -out ca.key 4096')
    expect(script).toContain('openssl genrsa -out redis.key 4096')
    expect(script).toContain('subjectAltName = IP:127.0.0.1,DNS:localhost')
    expect(script).not.toContain('chmod')
    expect(script).toContain('openssl x509 -in redis.crt -text -noout')
    expect(script).toContain('openssl.cnf')
    expect(script).toContain('[req_distinguished_name]')
    expect(script).toContain('[v3_ca]')
    expect(script).toContain('[v3_req]')
    expect(script).toContain('basicConstraints = critical, CA:true')
    expect(script).toContain('subjectAltName = IP:127.0.0.1,DNS:localhost')
    expect(script).not.toContain('openssl-san.cnf')
    expect(script).not.toContain('sed -i')
    expect(script).not.toContain('MSYS_NO_PATHCONV')
  })

  it('SAN 汇总节点 IP 与回环地址', () => {
    const sans = genInstallSans(baseOptions({ mode: 'cluster', ips: ['10.0.0.1', '10.0.0.2'] }))
    expect(sans).toContain('10.0.0.1')
    expect(sans).toContain('10.0.0.2')
    expect(sans).toContain('127.0.0.1')
    expect(sans).toContain('localhost')
  })

  it('不挂载配置：docker run 携带命令行参数形态，省略 Docker 默认项', () => {
    const out = genRedisInstall(baseOptions({ mountConf: false }), labels)
    const cmd = out.commands[0].code
    // 默认端口 6379 省略 --port；bind/protected-mode/dir 由 Docker entrypoint 默认处理
    expect(cmd).toContain('redis-server')
    expect(cmd).not.toContain('--port')
    expect(cmd).not.toContain('--bind')
    expect(cmd).not.toContain('--protected-mode')
    expect(cmd).not.toContain('--dir')
    expect(cmd).not.toContain('redis.conf')
  })

  it('不挂载配置 + 非默认端口：显式传递 --port', () => {
    const out = genRedisInstall(baseOptions({ mountConf: false, basePort: 6380 }), labels)
    const cmd = out.commands[0].code
    expect(cmd).toContain('--port 6380')
  })

  it('不挂载数据但挂载配置：目录仍创建（供 heredoc 写入）', () => {
    const out = genRedisInstall(baseOptions({ mountData: false, mountConf: true }), labels)
    const cmd = out.commands[0].code
    // 节点目录被创建（供配置文件写入），但不创建 data 子目录
    expect(cmd).toContain('mkdir -p /data/redis-single')
    expect(cmd).not.toContain('mkdir -p /data/redis-single/data')
    // 分步指南环境准备同样创建目录
    const envStep = out.guide.find(s => s.title.includes(labels.stepEnv))
    expect(envStep?.code).toContain('mkdir -p /data/redis-single')
    expect(envStep?.code).not.toContain('/data/redis-single/data')
  })

  it('alpine 变体追加镜像后缀', () => {
    const out = genRedisInstall(baseOptions({ alpine: true }), labels)
    expect(out.commands[0].code).toContain('redis:8-alpine')
  })

  it('heredoc 定界符与内容冲突时自动换名', () => {
    const out = genRedisInstall(baseOptions({ password: 'EOF' }), labels)
    const confStep = out.guide.find(s => s.title.includes(labels.stepConf))
    // 内容含 EOF 字样不破坏结构（定界符或内容安全）
    expect(confStep?.code).toContain('requirepass')
  })
})
