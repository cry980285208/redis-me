import { describe, expect, it } from 'vite-plus/test'

import { isReadonlyCommand, parseCommandName } from '@/locales/cmd'

describe('parseCommandName', () => {
  it('空命令', () => {
    expect(parseCommandName('')).toBe('')
    expect(parseCommandName('   ')).toBe('')
  })

  it('优先匹配最长已知前缀，大小写不敏感', () => {
    expect(parseCommandName('get mykey')).toBe('GET')
    expect(parseCommandName('ACL GETUSER alice')).toBe('ACL GETUSER')
    expect(parseCommandName('acl getuser')).toBe('ACL GETUSER')
    expect(parseCommandName('config get maxmemory')).toBe('CONFIG GET')
  })

  it('未知命令退回第一个词', () => {
    expect(parseCommandName('FOO BAR BAZ')).toBe('FOO')
  })
})

describe('isReadonlyCommand', () => {
  it('只读、写入、灰区、黑名单和未知命令', () => {
    expect(isReadonlyCommand('')).toBe(false)
    expect(isReadonlyCommand('  GET mykey  ')).toBe(true)
    expect(isReadonlyCommand('BITCOUNT k')).toBe(true)
    expect(isReadonlyCommand('SET k v')).toBe(false)
    expect(isReadonlyCommand('PING')).toBe(true)
    expect(isReadonlyCommand('CONFIG GET *')).toBe(true)
    expect(isReadonlyCommand('CONFIG SET a b')).toBe(false)
    expect(isReadonlyCommand('ACL GETUSER default')).toBe(true)
    expect(isReadonlyCommand('ACL SETUSER x on')).toBe(false)
    expect(isReadonlyCommand('CLIENT SETNAME app')).toBe(false)
    expect(isReadonlyCommand('EVAL "return 1" 0')).toBe(false)
    expect(isReadonlyCommand('MULTI')).toBe(false)
    expect(isReadonlyCommand('SCRIPT LOAD abc')).toBe(false)
    expect(isReadonlyCommand('NOPE')).toBe(false)
    expect(isReadonlyCommand('FT.SEARCH idx *')).toBe(true)
    expect(isReadonlyCommand('FT.INFO idx')).toBe(true)
    expect(isReadonlyCommand('FT._LIST')).toBe(true)
    expect(isReadonlyCommand('FT.CREATE idx ON HASH PREFIX 1 u: SCHEMA t TEXT')).toBe(false)
    expect(isReadonlyCommand('FT.DROPINDEX idx')).toBe(false)
  })
})
