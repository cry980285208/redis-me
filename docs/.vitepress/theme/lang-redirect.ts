/** localStorage 键：用户手动切换语言或自动跳转后写入，优先于浏览器语言 */
export const LANG_STORAGE_KEY = 'redisme-docs-lang'

/** 根路径访问时按偏好 / 浏览器语言跳转中文首页（inline script，尽早执行） */
export function createLangRedirectScript(base: string): string {
  const normalizedBase = base === '/' ? '/' : base.endsWith('/') ? base : `${base}/`
  const zhHome = normalizedBase === '/' ? '/zh/' : `${normalizedBase}zh/`

  return `(function(){try{
var k=${JSON.stringify(LANG_STORAGE_KEY)};
var base=${JSON.stringify(normalizedBase)};
var zh=${JSON.stringify(zhHome)};
var p=location.pathname;
if(p.endsWith('/index.html'))p=p.slice(0,-11)||'/';
var root=base==='/'?'/':base.replace(/\\/$/,'');
if(p!==root&&p!==root+'/')return;
var s=localStorage.getItem(k);
if(s==='zh'){location.replace(zh);return}
if(s==='en')return;
var l=(navigator.language||navigator.userLanguage||'').toLowerCase();
if(l.startsWith('zh'))location.replace(zh)
}catch(e){}})();`
}
