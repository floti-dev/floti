import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import '@helm/ui/styles.css'
import type { HelmBridge } from '../../../shared/ipc'
import { LayerRoot } from './LayerRoot'

/**
 * The browser's menu layer page (`main/menuLayer.ts`). It reaches nothing but
 * its own two channels: what to draw, from main, and what happened, back.
 * It opens no host connection, so it does not use the window's bridges.
 */
const container = document.getElementById('root')
if (!container) throw new Error('#root is missing from layer.html')

const bridge = (window as unknown as { helm: HelmBridge }).helm

createRoot(container).render(
  <StrictMode>
    <LayerRoot bridge={bridge} />
  </StrictMode>
)
