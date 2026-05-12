// widget.tsx — standalone secondary entry point.
//
// This module is loaded only on the /dashboard page. It is a separate Rollup
// chunk from main.tsx, so visitors to other pages never download this code.
//
// Mount target: <div id="widget-root"> in dashboard.html.

import { StrictMode, useEffect, useState } from 'react'
import { createRoot } from 'react-dom/client'

function LiveClock() {
  const [time, setTime] = useState(() => new Date().toLocaleTimeString())

  useEffect(() => {
    const id = setInterval(() => setTime(new Date().toLocaleTimeString()), 1000)
    return () => clearInterval(id)
  }, [])

  return (
    <div style={{ fontFamily: 'monospace', fontSize: '2rem', padding: '1rem' }}>
      {time}
    </div>
  )
}

const root = document.getElementById('widget-root')
if (root) {
  createRoot(root).render(
    <StrictMode>
      <LiveClock />
    </StrictMode>,
  )
}
