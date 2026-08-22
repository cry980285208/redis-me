/** main.ts 注入 buffer polyfill（php-serialize 的 decode 依赖全局 Buffer）；var 声明使其成为 globalThis 属性 */
declare var Buffer: typeof import('buffer').Buffer
declare type Buffer = import('buffer').Buffer

declare module '*.vue' {
  import type { DefineComponent } from 'vue'
  const component: DefineComponent<object, object, unknown>
  export default component
}

declare module '~virtual/svg-component' {
  import type { DefineComponent } from 'vue'
  const component: DefineComponent<object, object, unknown> & { name: string }
  export default component
}
