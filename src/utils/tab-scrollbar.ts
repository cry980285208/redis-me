// 多TAB 滚动条：与 Element Plus 的左右切换箭头同步出现。
//
// 背景：EP 的 el-tabs 用 transform 平移 nav（内部状态 navOffset）实现滚动，
// 只有内容溢出（nav 宽 > 可视容器宽）时才会显示左右切换箭头。
// 这里复用完全相同的溢出判定来决定滚动条是否出现，
// 并通过 wheel 事件驱动 EP 的 navOffset（EP 原生支持滚轮横向滚动），
// 保证「左右箭头」与「滚动条」两套滚动状态始终一致，不会各滚各的。
import { onBeforeUnmount, onMounted, ref, watch, type Ref } from 'vue'

/** 从 el-tabs 实例里取出被 transform 平移的 nav 与可视容器 */
function getNavEls(tabs: unknown): { nav: HTMLElement | null; scroll: HTMLElement | null } {
  const root = (tabs as { $el?: HTMLElement } | undefined)?.$el
  if (!root) return { nav: null, scroll: null }
  return {
    nav: root.querySelector<HTMLElement>('.el-tabs__nav'),
    scroll: root.querySelector<HTMLElement>('.el-tabs__nav-scroll'),
  }
}

/**
 * 多TAB 滚动条状态机
 * @param tabsRef el-tabs 组件实例的 ref
 */
export function useTabScrollbar(tabsRef: Ref<unknown>) {
  /** 是否溢出（与 EP 左右箭头出现的条件完全一致） */
  const scrollable = ref(false)
  /** 滑块宽度（百分比） */
  const thumbWidth = ref(0)
  /** 滑块左边距（百分比） */
  const thumbLeft = ref(0)

  let nav: HTMLElement | null = null
  let scroll: HTMLElement | null = null
  let resizeObserver: ResizeObserver | null = null
  let styleObserver: MutationObserver | null = null

  /** 读 EP 当前的滚动偏移（nav 的 transform 平移量） */
  function getOffset(): number {
    if (!nav) return 0
    const transform = getComputedStyle(nav).transform
    if (!transform || transform === 'none') return 0
    try {
      return Math.max(0, -new DOMMatrixReadOnly(transform).m41)
    } catch {
      return 0
    }
  }

  /** 重新计算溢出状态与滑块位置 */
  function update(): void {
    if (!nav || !scroll) return
    const navSize = nav.getBoundingClientRect().width
    const containerSize = scroll.getBoundingClientRect().width
    const overflow = containerSize < navSize
    scrollable.value = overflow
    if (!overflow) {
      thumbWidth.value = 0
      thumbLeft.value = 0
      return
    }
    const maxOffset = navSize - containerSize
    const offset = Math.min(getOffset(), maxOffset)
    thumbWidth.value = Math.max((containerSize / navSize) * 100, 8)
    thumbLeft.value = maxOffset > 0 ? (offset / maxOffset) * (100 - thumbWidth.value) : 0
  }

  /** 用 wheel 事件驱动 EP 的 navOffset（EP 原生监听 nav 的 wheel） */
  function scrollBy(delta: number): void {
    if (!nav || !delta) return
    nav.dispatchEvent(
      new WheelEvent('wheel', { deltaX: delta, deltaY: 0, bubbles: true, cancelable: true }),
    )
  }

  /** 按下滑块/轨道：拖动滑块，或点击轨道直接跳转 */
  function onThumbPointerDown(e: PointerEvent, track: HTMLElement | null): void {
    if (!track || !nav || !scroll) return
    e.preventDefault()
    e.stopPropagation()
    const trackRect = track.getBoundingClientRect()
    const navSize = nav.getBoundingClientRect().width
    const containerSize = scroll.getBoundingClientRect().width
    const maxOffset = navSize - containerSize
    if (maxOffset <= 0 || trackRect.width <= 0) return
    const thumbPx = (trackRect.width * thumbWidth.value) / 100
    const thumbPxLeft = (trackRect.width * thumbLeft.value) / 100
    // 点在轨道上（不是滑块）：直接跳到点击处
    const onThumb = (e.target as HTMLElement | null)?.classList.contains('tab-scrollbar__thumb')
    if (!onThumb) {
      const ratio = Math.min(Math.max((e.clientX - trackRect.left) / trackRect.width, 0), 1)
      scrollBy(ratio * maxOffset - getOffset())
      return
    }
    // 按下点在滑块内的相对位置，拖动时保持不变（避免滑块跳到按下处）
    const grab = thumbPx > 0 ? Math.min(Math.max((e.clientX - trackRect.left - thumbPxLeft) / thumbPx, 0), 1) : 0
    const onMove = (ev: PointerEvent): void => {
      const x = ev.clientX - trackRect.left - grab * thumbPx
      const movable = Math.max(trackRect.width - thumbPx, 1)
      const ratio = Math.min(Math.max(x / movable, 0), 1)
      scrollBy(ratio * maxOffset - getOffset())
    }
    const onUp = (): void => {
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
    }
    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUp)
  }

  function observe(): void {
    const els = getNavEls(tabsRef.value)
    nav = els.nav
    scroll = els.scroll
    resizeObserver?.disconnect()
    styleObserver?.disconnect()
    if (!nav || !scroll) return
    // 页签增减 / 窗口尺寸变化 → 溢出状态与滑块位置都要重算
    resizeObserver = new ResizeObserver(() => update())
    resizeObserver.observe(nav)
    resizeObserver.observe(scroll)
    // EP 滚动时会改 nav 的 transform，跟着它同步滑块
    styleObserver = new MutationObserver(() => update())
    styleObserver.observe(nav, { attributes: true, attributeFilter: ['style'] })
    update()
  }

  onMounted(() => {
    observe()
    window.addEventListener('resize', update)
  })
  onBeforeUnmount(() => {
    window.removeEventListener('resize', update)
    resizeObserver?.disconnect()
    styleObserver?.disconnect()
  })
  watch(tabsRef, observe)

  return { scrollable, thumbWidth, thumbLeft, onThumbPointerDown }
}
