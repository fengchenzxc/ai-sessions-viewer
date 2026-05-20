import { createApp } from 'vue'
import './style.css'
import App from './App.vue'
import { vTooltip } from './tooltip'

if (navigator.platform.startsWith('Mac')) {
  document.documentElement.classList.add('is-macos')
}

createApp(App).directive('tooltip', vTooltip).mount('#app')
