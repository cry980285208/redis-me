<script setup lang="ts">
import SvgIcon from '~virtual/svg-component'
</script>

<template>
  <!-- el-select :suffix-icon 只能收组件引用，故单独抽出（配合 .me-select-plain） -->
  <SvgIcon name="me-icon-upDown" class="me-select-plain-arrow" />
</template>
