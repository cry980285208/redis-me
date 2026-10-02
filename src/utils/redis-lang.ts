/** FT.CREATE 高亮。只覆盖 DDL 里会出现的词，颜色在浅色和深色背景上都能看清。 */

import {
  HighlightStyle,
  LanguageSupport,
  StreamLanguage,
  syntaxHighlighting,
} from '@codemirror/language'
import { tags } from '@lezer/highlight'

const REDIS_KEYWORDS = new Set([
  'FT.CREATE',
  'ON',
  'PREFIX',
  'FILTER',
  'LANGUAGE',
  'LANGUAGE_FIELD',
  'SCORE',
  'SCORE_FIELD',
  'PAYLOAD_FIELD',
  'STOPWORDS',
  'INDEXALL',
  'SCHEMA',
  'AS',
  'NOOFFSETS',
  'NOHL',
  'NOFIELDS',
  'NOFREQS',
  'MAXTEXTFIELDS',
  'TEMPORARY',
  'SKIPINITIALSCAN',
])

const REDIS_ARGS = new Set([
  'HASH',
  'JSON',
  'TEXT',
  'TAG',
  'NUMERIC',
  'GEO',
  'GEOSHAPE',
  'VECTOR',
  'FLAT',
  'HNSW',
  'SVS',
  // 'NOSTEM',  // 手动注释掉，以便: TEXT NOSETM SORTABLE中间的NOSTEM颜色不一致，和RedisInsight类似
  'SORTABLE',
  'UNF',
  'NOINDEX',
  'WITHSUFFIXTRIE',
  'INDEXEMPTY',
  'INDEXMISSING',
  'CASESENSITIVE',
  'TYPE',
  'FLOAT32',
  'FLOAT64',
  'FLOAT16',
  'BFLOAT16',
  'DIM',
  'DISTANCE_METRIC',
  'L2',
  'COSINE',
  'IP',
  'INITIAL_CAP',
  'BLOCK_SIZE',
  'M',
  'EF_CONSTRUCTION',
  'EF_RUNTIME',
  'EPSILON',
  'WEIGHT',
  'SEPARATOR',
  'PHONETIC',
  'ENABLE',
  'DISABLE',
])

type RedisHighlightState = { indexDone: boolean; words: number }

/** 同一次调用结果要复用。tags.special 每次都会新建 tag，高亮规则对不上。 */
const indexTag = tags.special(tags.variableName)

/**
 * 不用 themeType。编辑器没标成 dark 时会退回默认高亮：
 * typeName 是 #164，深色背景上看不见，浅色又和正文差不多。
 * 颜色取两种背景都能分开的一档。
 */
const redisHighlight = HighlightStyle.define([
  { tag: tags.keyword, color: '#409eff' }, // 关键字
  { tag: indexTag, color: '#67C23A' }, // 索引名
  { tag: tags.typeName, color: '#E6A23C' }, // 参数名
  { tag: tags.string, color: '#909399' }, // 字符串
])

/** 必须作为非 fallback 高亮挂上，否则默认高亮会盖住 TEXT / NOSTEM 这类词。 */
export const redisHighlighting = syntaxHighlighting(redisHighlight)

export const redisLang = new LanguageSupport(
  StreamLanguage.define<RedisHighlightState>({
    startState: () => ({ indexDone: false, words: 0 }),
    token(stream, state) {
      if (stream.sol()) state.words = 0
      if (stream.eatSpace()) return null
      if (stream.peek() === '"') {
        stream.next()
        while (!stream.eol()) {
          const ch = stream.next()
          if (ch === '\\') {
            stream.next()
            continue
          }
          if (ch === '"') break
        }
        return 'string'
      }
      stream.match(/[^\s"]+/)
      const word = stream.current().toUpperCase()
      state.words++
      if (!state.indexDone && state.words === 1 && word === 'FT.CREATE') return 'keyword'
      if (!state.indexDone && state.words === 2) {
        state.indexDone = true
        return 'index'
      }
      if (REDIS_KEYWORDS.has(word)) return 'keyword'
      if (REDIS_ARGS.has(word)) return 'arg'
      return null
    },
    tokenTable: { keyword: tags.keyword, index: indexTag, arg: tags.typeName, string: tags.string },
  }),
  [redisHighlighting],
)
