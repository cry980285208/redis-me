import { describe, expect, it } from 'vitest'

import { formatJavaSerDisplay, javaSerBase64ToValue, normalizeJavaObject } from '@/utils/javaserial'

/** 由 JDK ObjectOutputStream 生成的固定样例 */
const SAMPLES = {
  linkedmap:
    'rO0ABXNyABdqYXZhLnV0aWwuTGlua2VkSGFzaE1hcDTATlwQbMD7AgABWgALYWNjZXNzT3JkZXJ4cgARamF2YS51dGlsLkhhc2hNYXAFB9rBwxZg0QMAAkYACmxvYWRGYWN0b3JJAAl0aHJlc2hvbGR4cD9AAAAAAAAMdwgAAAAQAAAAAnQAAWFzcgARamF2YS5sYW5nLkludGVnZXIS4qCk94GHOAIAAUkABXZhbHVleHIAEGphdmEubGFuZy5OdW1iZXKGrJUdC5TgiwIAAHhwAAAAAXQAAWJ0AAPkuox4AA==',
  linkedset:
    'rO0ABXNyABdqYXZhLnV0aWwuTGlua2VkSGFzaFNldNhs11qV3SoeAgAAeHIAEWphdmEudXRpbC5IYXNoU2V0ukSFlZa4tzQDAAB4cHcMAAAAED9AAAAAAAACdAABeHQAAXl4',
  treemap:
    'rO0ABXNyABFqYXZhLnV0aWwuVHJlZU1hcAzB9j4tJWrmAwABTAAKY29tcGFyYXRvcnQAFkxqYXZhL3V0aWwvQ29tcGFyYXRvcjt4cHB3BAAAAAJ0AAFhc3IAEWphdmEubGFuZy5JbnRlZ2VyEuKgpPeBhzgCAAFJAAV2YWx1ZXhyABBqYXZhLmxhbmcuTnVtYmVyhqyVHQuU4IsCAAB4cAAAAAF0AAFic3EAfgAEAAAAAng=',
  treeset: 'rO0ABXNyABFqYXZhLnV0aWwuVHJlZVNldN2YUJOV7YdbAwAAeHBwdwQAAAACdAABYXQAAWJ4',
  concurrentmap:
    'rO0ABXNyACZqYXZhLnV0aWwuY29uY3VycmVudC5Db25jdXJyZW50SGFzaE1hcGSZ3hKdhyk9AwADSQALc2VnbWVudE1hc2tJAAxzZWdtZW50U2hpZnRbAAhzZWdtZW50c3QAMVtMamF2YS91dGlsL2NvbmN1cnJlbnQvQ29uY3VycmVudEhhc2hNYXAkU2VnbWVudDt4cAAAAA8AAAAcdXIAMVtMamF2YS51dGlsLmNvbmN1cnJlbnQuQ29uY3VycmVudEhhc2hNYXAkU2VnbWVudDtSdz9BMps5dAIAAHhwAAAAEHNyAC5qYXZhLnV0aWwuY29uY3VycmVudC5Db25jdXJyZW50SGFzaE1hcCRTZWdtZW50HzZMkFiTKT0CAAFGAApsb2FkRmFjdG9yeHIAKGphdmEudXRpbC5jb25jdXJyZW50LmxvY2tzLlJlZW50cmFudExvY2tmVagsLMhq6wIAAUwABHN5bmN0AC9MamF2YS91dGlsL2NvbmN1cnJlbnQvbG9ja3MvUmVlbnRyYW50TG9jayRTeW5jO3hwc3IANGphdmEudXRpbC5jb25jdXJyZW50LmxvY2tzLlJlZW50cmFudExvY2skTm9uZmFpclN5bmNliDLnU3u/CwIAAHhyAC1qYXZhLnV0aWwuY29uY3VycmVudC5sb2Nrcy5SZWVudHJhbnRMb2NrJFN5bmO4HqKUqkRafAIAAHhyADVqYXZhLnV0aWwuY29uY3VycmVudC5sb2Nrcy5BYnN0cmFjdFF1ZXVlZFN5bmNocm9uaXplcmZVqEN1P1LjAgABSQAFc3RhdGV4cgA2amF2YS51dGlsLmNvbmN1cnJlbnQubG9ja3MuQWJzdHJhY3RPd25hYmxlU3luY2hyb25pemVyM9+vua1tb6kCAAB4cAAAAAA/QAAAc3EAfgAFc3EAfgAJAAAAAD9AAABzcQB+AAVzcQB+AAkAAAAAP0AAAHNxAH4ABXNxAH4ACQAAAAA/QAAAc3EAfgAFc3EAfgAJAAAAAD9AAABzcQB+AAVzcQB+AAkAAAAAP0AAAHNxAH4ABXNxAH4ACQAAAAA/QAAAc3EAfgAFc3EAfgAJAAAAAD9AAABzcQB+AAVzcQB+AAkAAAAAP0AAAHNxAH4ABXNxAH4ACQAAAAA/QAAAc3EAfgAFc3EAfgAJAAAAAD9AAABzcQB+AAVzcQB+AAkAAAAAP0AAAHNxAH4ABXNxAH4ACQAAAAA/QAAAc3EAfgAFc3EAfgAJAAAAAD9AAABzcQB+AAVzcQB+AAkAAAAAP0AAAHNxAH4ABXNxAH4ACQAAAAA/QAAAc3EAfgAFc3EAfgAJAAAAAD9AAABzcQB+AAVzcQB+AAkAAAAAP0AAAHNxAH4ABXNxAH4ACQAAAAA/QAAAdAADY2sxdAADY3YxdAADY2syc3IAEWphdmEubGFuZy5JbnRlZ2VyEuKgpPeBhzgCAAFJAAV2YWx1ZXhyABBqYXZhLmxhbmcuTnVtYmVyhqyVHQuU4IsCAAB4cAAAAAlwcHg=',
  enummap:
    'rO0ABXNyABFqYXZhLnV0aWwuRW51bU1hcAZdffe+kHyhAwABTAAHa2V5VHlwZXQAEUxqYXZhL2xhbmcvQ2xhc3M7eHB2cgAOR2VuVml0ZXN0JFJvbGUAAAAAAAAAABIAAHhyAA5qYXZhLmxhbmcuRW51bQAAAAAAAAAAEgAAeHB3BAAAAAJ+cQB+AAN0AAVBRE1JTnQAAWF+cQB+AAN0AARVU0VSdAABdXg=',
  enumset:
    'rO0ABXNyACRqYXZhLnV0aWwuRW51bVNldCRTZXJpYWxpemF0aW9uUHJveHkFB9PbdlTK0QIAAkwAC2VsZW1lbnRUeXBldAARTGphdmEvbGFuZy9DbGFzcztbAAhlbGVtZW50c3QAEVtMamF2YS9sYW5nL0VudW07eHB2cgAOR2VuVml0ZXN0JFJvbGUAAAAAAAAAABIAAHhyAA5qYXZhLmxhbmcuRW51bQAAAAAAAAAAEgAAeHB1cgARW0xqYXZhLmxhbmcuRW51bTuojeotM9IvmAIAAHhwAAAAAn5xAH4ABHQABUFETUlOfnEAfgAEdAAFR1VFU1Q=',
  vector:
    'rO0ABXNyABBqYXZhLnV0aWwuVmVjdG9y2Zd9W4A7rwEDAANJABFjYXBhY2l0eUluY3JlbWVudEkADGVsZW1lbnRDb3VudFsAC2VsZW1lbnREYXRhdAATW0xqYXZhL2xhbmcvT2JqZWN0O3hwAAAAAAAAAAJ1cgATW0xqYXZhLmxhbmcuT2JqZWN0O5DOWJ8QcylsAgAAeHAAAAAKdAACdjF0AAJ2MnBwcHBwcHBweA==',
  deque: 'rO0ABXNyABRqYXZhLnV0aWwuQXJyYXlEZXF1ZSB82i4kDaCLAwAAeHB3BAAAAAJ0AAJkMXQAAmQyeA==',
  bitset:
    'rO0ABXNyABBqYXZhLnV0aWwuQml0U2V0bv2Ifjk0qyEDAAFbAARiaXRzdAACW0p4cHVyAAJbSnggBLUSsXWTAgAAeHAAAAABAAAAAAAAAQp4',
  sbuilder:
    'rO0ABXNyABdqYXZhLmxhbmcuU3RyaW5nQnVpbGRlcjzV+xRaTGrLAwAAeHB3BAAAAAV1cgACW0OwJmaw4l2ErAIAAHhwAAAAFQBoAGUAbABsAG8AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAHg=',
  inet: 'rO0ABXNyABRqYXZhLm5ldC5JbmV0QWRkcmVzcy2bV6+f4+vbAwADSQAHYWRkcmVzc0kABmZhbWlseUwACGhvc3ROYW1ldAASTGphdmEvbGFuZy9TdHJpbmc7eHB/AAABAAAAAnB4',
  sqldate:
    'rO0ABXNyAA1qYXZhLnNxbC5EYXRlFPpGaD81ZpcCAAB4cgAOamF2YS51dGlsLkRhdGVoaoEBS1l0GQMAAHhwdwgAAAGPz2EgAHg=',
  biginteger:
    'rO0ABXNyABRqYXZhLm1hdGguQmlnSW50ZWdlcoz8nx+pO/sdAwAGSQAIYml0Q291bnRJAAliaXRMZW5ndGhJABNmaXJzdE5vbnplcm9CeXRlTnVtSQAMbG93ZXN0U2V0Qml0SQAGc2lnbnVtWwAJbWFnbml0dWRldAACW0J4cgAQamF2YS5sYW5nLk51bWJlcoaslR0LlOCLAgAAeHD///////////////7////+AAAAAXVyAAJbQqzzF/gGCFTgAgAAeHAAAAAJBVqlTTjlJn7qeA==',
  period: 'rO0ABXNyAA1qYXZhLnRpbWUuU2VylV2EuhsiSLIMAAB4cHcNDv////8AAAAC/////Xg=',
  instant: 'rO0ABXNyAA1qYXZhLnRpbWUuU2VylV2EuhsiSLIMAAB4cHcNAv//////////AAAAAHg=',
} as const

describe('javaserial', () => {
  it('normalize 循环引用不栈溢出', () => {
    const a: Record<string, unknown> = { $class: 'A' }
    const b: Record<string, unknown> = { $class: 'B' }
    a.b = b
    b.a = a
    const out = normalizeJavaObject(a) as Record<string, unknown>
    expect(out.$class).toBe('A')
    expect((out.b as Record<string, unknown>).$class).toBe('B')
    expect((out.b as Record<string, unknown>).a).toEqual({ $ref: 'circular' })
  })

  it('format 保留 NaN/Infinity 字面量', () => {
    expect(formatJavaSerDisplay(Number.NaN)).toContain('NaN')
    expect(formatJavaSerDisplay(Number.POSITIVE_INFINITY)).toContain('Infinity')
  })

  it('解码常见 Map / Set', () => {
    expect(javaSerBase64ToValue(SAMPLES.linkedmap)).toEqual({ a: 1, b: '二' })
    expect(javaSerBase64ToValue(SAMPLES.treemap)).toEqual({ a: 1, b: 2 })
    expect(javaSerBase64ToValue(SAMPLES.concurrentmap)).toEqual({ ck1: 'cv1', ck2: 9 })
    expect(javaSerBase64ToValue(SAMPLES.linkedset)).toEqual(['x', 'y'])
    expect(javaSerBase64ToValue(SAMPLES.treeset)).toEqual(['a', 'b'])
    expect(javaSerBase64ToValue(SAMPLES.enummap)).toEqual({
      $type: 'java.util.EnumMap',
      value: { ADMIN: 'a', USER: 'u' },
    })
    expect(javaSerBase64ToValue(SAMPLES.enumset)).toEqual({
      $type: 'java.util.EnumSet',
      value: ['ADMIN', 'GUEST'],
    })
  })

  it('解码 Vector / ArrayDeque / BitSet / StringBuilder / Inet / sql.Date / BigInteger', () => {
    expect(javaSerBase64ToValue(SAMPLES.vector)).toEqual(['v1', 'v2'])
    expect(javaSerBase64ToValue(SAMPLES.deque)).toEqual(['d1', 'd2'])
    expect(javaSerBase64ToValue(SAMPLES.bitset)).toEqual({
      $type: 'java.util.BitSet',
      value: [1, 3, 8],
    })
    expect(javaSerBase64ToValue(SAMPLES.sbuilder)).toEqual({
      $type: 'java.lang.StringBuilder',
      value: 'hello',
    })
    expect(javaSerBase64ToValue(SAMPLES.inet)).toEqual({
      $type: 'java.net.InetAddress',
      value: '127.0.0.1',
    })
    expect(javaSerBase64ToValue(SAMPLES.sqldate)).toEqual({
      $type: 'java.sql.Date',
      value: '2024-06-01',
    })
    expect(javaSerBase64ToValue(SAMPLES.biginteger)).toEqual({
      $type: 'java.math.BigInteger',
      value: '98765432109876543210',
    })
  })

  it('Period / Instant 负值按 signed 展示', () => {
    expect(javaSerBase64ToValue(SAMPLES.period)).toEqual({
      $type: 'java.time.Period',
      years: -1,
      months: 2,
      days: -3,
    })
    expect(javaSerBase64ToValue(SAMPLES.instant)).toEqual({
      $type: 'java.time.Instant',
      value: '1969-12-31T23:59:59.000Z',
    })
  })
})
