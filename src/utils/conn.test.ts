import { describe, expect, it } from 'vite-plus/test'

import type { UiConn } from '@/types/me-interface'
import { buildRedisUrl, isConnMinimalMode } from '@/utils/conn'

describe('buildRedisUrl', () => {
  it('基本单机：redis://host:port', () => {
    expect(buildRedisUrl({ host: '127.0.0.1', port: 6379 })).toBe('redis://127.0.0.1:6379')
  })

  it('TLS：rediss:// 协议', () => {
    expect(buildRedisUrl({ host: 'redis.example.com', port: 6380, ssl: true })).toBe(
      'rediss://redis.example.com:6380',
    )
  })

  it('含用户名和密码', () => {
    expect(
      buildRedisUrl({ host: '10.0.0.1', port: 6379, username: 'admin', password: 's3cret' }),
    ).toBe('redis://admin:s3cret@10.0.0.1:6379')
  })

  it('仅密码无用户名', () => {
    expect(buildRedisUrl({ host: '10.0.0.1', port: 6379, password: 'pass' })).toBe(
      'redis://:pass@10.0.0.1:6379',
    )
  })

  it('密码含特殊字符自动编码', () => {
    expect(
      buildRedisUrl({ host: '10.0.0.1', port: 6379, username: 'user', password: 'p@ss:w/rd' }),
    ).toBe('redis://user:p%40ss%3Aw%2Frd@10.0.0.1:6379')
  })

  it('db > 0 时追加路径', () => {
    expect(buildRedisUrl({ host: '127.0.0.1', port: 6379, db: 3 })).toBe('redis://127.0.0.1:6379/3')
  })

  it('db = 0 时不追加路径', () => {
    expect(buildRedisUrl({ host: '127.0.0.1', port: 6379, db: 0 })).toBe('redis://127.0.0.1:6379')
  })

  it('IPv6 地址自动加方括号', () => {
    expect(buildRedisUrl({ host: '::1', port: 6379 })).toBe('redis://[::1]:6379')
  })

  it('已有方括号的 IPv6 不重复加', () => {
    expect(buildRedisUrl({ host: '[::1]', port: 6379 })).toBe('redis://[::1]:6379')
  })

  it('空 host 默认 127.0.0.1', () => {
    expect(buildRedisUrl({ host: '', port: 6379 })).toBe('redis://127.0.0.1:6379')
  })

  it('RESP3 协议追加查询参数', () => {
    expect(buildRedisUrl({ host: '127.0.0.1', port: 6379, protocol: 'resp3' })).toBe(
      'redis://127.0.0.1:6379?protocol=resp3',
    )
  })

  it('RESP3 + db + TLS 组合', () => {
    expect(
      buildRedisUrl({ host: '10.0.0.1', port: 6380, ssl: true, db: 2, protocol: 'resp3' }),
    ).toBe('rediss://10.0.0.1:6380/2?protocol=resp3')
  })

  it('resp2 不追加查询参数', () => {
    expect(buildRedisUrl({ host: '127.0.0.1', port: 6379, protocol: 'resp2' })).toBe(
      'redis://127.0.0.1:6379',
    )
  })
})

describe('isConnMinimalMode', () => {
  const conn = (meta: Record<string, unknown> = {}) => ({ meta }) as unknown as UiConn

  it('meta.uiMode=minimal 为真', () => {
    expect(isConnMinimalMode(conn({ uiMode: 'minimal' }))).toBe(true)
  })

  it('缺省或其它值不为极简', () => {
    expect(isConnMinimalMode(null)).toBe(false)
    expect(isConnMinimalMode(conn())).toBe(false)
    expect(isConnMinimalMode(conn({ uiMode: 'normal' }))).toBe(false)
  })
})
