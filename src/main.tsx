import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import App from './App'
import './App.css'
import './ui/Toolbar.css'
import './ui/FileExplorer.css'
import './ui/Editor.css'
import './ui/OutputPanel.css'
import './ui/AIAgents.css'
import './ui/StatusBar.css'
import './ui/Settings.css'
import './ui/ProjectDialog.css'
import './ui/SerialMonitor.css'
import './ui/EngineeringPanels.css'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
