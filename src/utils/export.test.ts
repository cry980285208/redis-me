import { describe, expect, it } from 'vite-plus/test'
import * as XLSX from 'xlsx'

import {
  buildExportFileName,
  buildTimestampedFileName,
  matrixToCsv,
  matrixToHtml,
  matrixToJson,
  matrixToMarkdown,
  matrixToTsv,
  matrixToXlsxBytes,
} from '@/utils/export'

const headers = ['name', 'note']
const rows = [
  ['plain', 'a,b'],
  ['say "hi"', 'x\ny'],
]

describe('export file names', () => {
  it('时间戳文件名', () => {
    expect(buildTimestampedFileName('pre', 'json')).toMatch(/^pre_\d{14}\.json$/)
    expect(buildTimestampedFileName('pre', 'txt', '-')).toMatch(/^pre-\d{14}\.txt$/)
    expect(buildExportFileName('Conn', 'mec')).toMatch(/^RedisME_Conn_\d{14}\.mec$/)
  })
})

describe('matrix export', () => {
  it('JSON：空表头用 columnN，缺单元格为空串', () => {
    expect(JSON.parse(matrixToJson(['', 'name'], [['v']]))).toEqual([{ column1: 'v', name: '' }])
  })

  it('CSV 转义逗号、引号和换行，并带 BOM', () => {
    const csv = matrixToCsv(headers, rows)
    expect(csv.charCodeAt(0)).toBe(0xfeff)
    expect(csv.slice(1)).toBe('name,note\nplain,"a,b"\n"say ""hi""","x\ny"')
  })

  it('HTML 转义并包成完整文档', () => {
    const html = matrixToHtml(['a&b'], [['<x>']])
    expect(html.startsWith('<!DOCTYPE html>')).toBe(true)
    expect(html).toContain('<th style="white-space:nowrap">a&amp;b</th>')
    expect(html).toContain('<td>&lt;x&gt;</td>')
    expect(matrixToHtml(['q'], [['a"b']])).toContain('<td>a&quot;b</td>')
    expect(html).not.toContain('<x>')
  })

  it('Markdown 转义竖线和反斜杠，单元格内换行变空格', () => {
    expect(matrixToMarkdown(['a|b', 'c'], [['x\\y\nz', '']])).toBe(
      '| a\\|b | c |\n| --- | --- |\n| x\\\\y z |  |',
    )
  })

  it('TSV 把 Tab 和换行换成空格，并带 BOM', () => {
    const tsv = matrixToTsv(['a', 'b'], [['x\ty', 'z']])
    expect(tsv.charCodeAt(0)).toBe(0xfeff)
    expect(tsv.slice(1)).toBe('a\tb\nx y\tz')
  })

  it('xlsx 能读回同一张表', () => {
    const bytes = matrixToXlsxBytes(headers, [['plain', 'a,b']])
    const book = XLSX.read(bytes, { type: 'array' })
    const sheet = book.Sheets[book.SheetNames[0]!]
    expect(XLSX.utils.sheet_to_json(sheet!, { header: 1 })).toEqual([headers, ['plain', 'a,b']])
  })
})
