import { defineComponent, h } from 'vue'
import SvgIcon from '~virtual/svg-component'

/** el-select 无边框下拉后缀：上下箭头，不随展开旋转（配合 .me-select-plain） */
export const MeSelectUpDownIcon = defineComponent({
  name: 'MeSelectUpDownIcon',
  setup() {
    return () => h(SvgIcon, { name: 'me-icon-upDown', class: 'me-select-plain-arrow' })
  },
})
