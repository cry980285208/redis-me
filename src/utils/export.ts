/** 文件导出：保存对话框、MeTable 矩阵格式转换、连接/表单等文本导出 */

import { save, type DialogFilter } from '@tauri-apps/plugin-dialog'
import { writeFile, writeTextFile } from '@tauri-apps/plugin-fs'
import dayjs from 'dayjs'
import * as XLSX from 'xlsx'

import i18n from '@/locales'
import { meCopy, meErr, meOk } from '@/utils/util'

const { t } = i18n.global

// #region 文件名与保存对话框

/** `prefix_YYYYMMDDHHmmss.ext`；MeFileInput、KeyBatch 导出路径 */
export function buildTimestampedFileName(prefix: string, ext: string, sep = '_'): string {
  return `${prefix}${sep}${dayjs().format('YYYYMMDDHHmmss')}.${ext}`
}

/** `RedisME_{name}_YYYYMMDDHHmmss.ext`；MeTable、TabConn 导出连接 */
export function buildExportFileName(namePart: string, ext: string): string {
  return buildTimestampedFileName(`RedisME_${namePart}`, ext)
}

/** 打开保存对话框；saveTextExport / saveBinaryExport 内部 */
export async function pickSavePath(
  defaultPath: string,
  extensions: string[],
  filterName?: string,
): Promise<string | null> {
  const filters: DialogFilter[] = [
    { name: filterName ?? extensions[0]?.toUpperCase() ?? 'File', extensions },
  ]
  return save({ defaultPath, filters })
}

type ExportMessages = { ok: string; err: string }

/** 选路径并写入文本；TabConn 导出 .mec */
export async function saveTextExport(
  content: string,
  defaultPath: string,
  extensions: string[],
  messages: ExportMessages = { ok: t('meTable.exportOk'), err: t('meTable.exportErr') },
  filterName?: string,
): Promise<void> {
  const path = await pickSavePath(defaultPath, extensions, filterName)
  if (!path) return
  try {
    await writeTextFile(path, content)
    meOk(messages.ok)
  } catch (e: unknown) {
    meErr(e instanceof Error ? e : String(e), messages.err)
  }
}

/** 选路径并写入二进制；MeTable 导出 xlsx */
export async function saveBinaryExport(
  bytes: Uint8Array,
  defaultPath: string,
  extensions: string[],
  messages: ExportMessages = { ok: t('meTable.exportOk'), err: t('meTable.exportErr') },
  filterName?: string,
): Promise<void> {
  const path = await pickSavePath(defaultPath, extensions, filterName)
  if (!path) return
  try {
    await writeFile(path, bytes)
    meOk(messages.ok)
  } catch (e: unknown) {
    meErr(e instanceof Error ? e : String(e), messages.err)
  }
}

// #endregion

// #region MeTable：矩阵格式转换

/** MeTable exportRows 返回值：表头 + 文本矩阵 */
export type TableExportMatrix = { headers: string[]; rows: string[][] }

export function matrixToJson(headers: string[], rows: string[][]): string {
  const objects = rows.map(row => {
    const obj: Record<string, string> = {}
    headers.forEach((header, index) => {
      obj[header || `column${index + 1}`] = row[index] ?? ''
    })
    return obj
  })
  return JSON.stringify(objects, null, 2)
}

function csvEscape(value: string): string {
  if (/[",\n\r]/.test(value)) return `"${value.replace(/"/g, '""')}"`
  return value
}

export function matrixToCsv(headers: string[], rows: string[][]): string {
  const lines = [headers.map(csvEscape).join(','), ...rows.map(row => row.map(csvEscape).join(','))]
  return `\ufeff${lines.join('\n')}`
}

function htmlEscape(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

function buildTableHtml(headers: string[], rows: string[][]): string {
  const headerHtml = headers.map(h => `<th>${htmlEscape(h)}</th>`).join('')
  const bodyHtml = rows
    .map(row => {
      const cells = row.map(cell => `<td>${htmlEscape(cell)}</td>`).join('')
      return `<tr>${cells}</tr>`
    })
    .join('')
  return `<table border="1" cellpadding="4" cellspacing="0" style="border-collapse:collapse;"><thead><tr>${headerHtml}</tr></thead><tbody>${bodyHtml}</tbody></table>`
}

function mdEscapeCell(value: string): string {
  return value.replace(/\\/g, '\\\\').replace(/\|/g, '\\|').replace(/\n/g, ' ')
}

export function matrixToMarkdown(headers: string[], rows: string[][]): string {
  const headerRow = `| ${headers.map(mdEscapeCell).join(' | ')} |`
  const separator = `| ${headers.map(() => '---').join(' | ')} |`
  const bodyRows = rows.map(row => {
    const cells = headers.map((_, index) => mdEscapeCell(row[index] ?? ''))
    return `| ${cells.join(' | ')} |`
  })
  return [headerRow, separator, ...bodyRows].join('\n')
}

/** 复制 HTML 表格（富文本 + Markdown 回退）；MeTable */
export async function copyTableHtml(headers: string[], rows: string[][]): Promise<void> {
  const html = buildTableHtml(headers, rows)
  const plain = matrixToMarkdown(headers, rows)
  try {
    await navigator.clipboard.write([
      new ClipboardItem({
        'text/html': new Blob([html], { type: 'text/html' }),
        'text/plain': new Blob([plain], { type: 'text/plain' }),
      }),
    ])
    meOk(t('copyOk'))
  } catch {
    meCopy(html)
  }
}

export function matrixToHtml(headers: string[], rows: string[][]): string {
  const table = buildTableHtml(headers, rows)
  return `<!DOCTYPE html>
<html>
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <style>
    body { font-family: system-ui, sans-serif; padding: 16px; }
    table { border-collapse: collapse; width: 100%; }
    th, td { border: 1px solid #ccc; padding: 6px 10px; text-align: left; }
    th { background: #f5f5f5; }
    tr:nth-child(even) { background: #fafafa; }
  </style>
</head>
<body>
  ${table}
</body>
</html>`
}

export function matrixToXlsxBytes(headers: string[], rows: string[][]): Uint8Array {
  const ws = XLSX.utils.aoa_to_sheet([headers, ...rows])
  const wb = XLSX.utils.book_new()
  XLSX.utils.book_append_sheet(wb, ws, 'Sheet1')
  const buf = XLSX.write(wb, { bookType: 'xlsx', type: 'array' })
  return new Uint8Array(buf)
}

/** MeTable 导出 json/csv/html/md */
export async function saveTableTextFile(
  content: string,
  defaultPath: string,
  extensions: string[],
): Promise<void> {
  await saveTextExport(content, defaultPath, extensions)
}

/** MeTable 导出 xlsx */
export async function saveTableXlsxFile(
  headers: string[],
  rows: string[][],
  defaultPath: string,
): Promise<void> {
  await saveBinaryExport(matrixToXlsxBytes(headers, rows), defaultPath, ['xlsx'], undefined, 'XLSX')
}

// #endregion
