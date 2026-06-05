import { createApp } from 'vue'
import './style.css'
import App from './App.vue'
import { vTooltip } from './tooltip'
import { openUrl } from './api'

if (navigator.platform.startsWith('Mac')) {
  document.documentElement.classList.add('is-macos')
}

document.addEventListener('click', (e) => {
  const a = (e.target as HTMLElement).closest('a[href]') as HTMLAnchorElement | null
  if (!a) return
  const href = a.getAttribute('href') ?? ''
  if (href.startsWith('http://') || href.startsWith('https://')) {
    e.preventDefault()
    openUrl(href)
  }
})

document.addEventListener('contextmenu', (e) => {
  const a = (e.target as HTMLElement).closest('a[href]') as HTMLAnchorElement | null
  if (a) e.preventDefault()
})

createApp(App).directive('tooltip', vTooltip).mount('#app')
