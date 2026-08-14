import React, { useCallback } from 'react'

import { Box } from '@chakra-ui/react'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'

type ResizeDirection =
  | 'East'
  | 'North'
  | 'NorthEast'
  | 'NorthWest'
  | 'South'
  | 'SouthEast'
  | 'SouthWest'
  | 'West'

const EDGE = 4
const CORNER = 6
/** Keep top-left clear so the header grip can start a move, not a resize. */
const HEADER_CLEARANCE = 40

const HANDLES: Array<{
  direction: ResizeDirection
  cursor: string
  style: React.CSSProperties
}> = [
  {
    direction: 'North',
    cursor: 'n-resize',
    style: {
      top: 0,
      left: HEADER_CLEARANCE,
      right: CORNER,
      height: EDGE,
    },
  },
  {
    direction: 'South',
    cursor: 's-resize',
    style: { bottom: 0, left: CORNER, right: CORNER, height: EDGE },
  },
  {
    direction: 'West',
    cursor: 'w-resize',
    style: {
      left: 0,
      top: HEADER_CLEARANCE,
      bottom: CORNER,
      width: EDGE,
    },
  },
  {
    direction: 'East',
    cursor: 'e-resize',
    style: { right: 0, top: CORNER, bottom: CORNER, width: EDGE },
  },
  // Skip NorthWest — conflicts with the drag grip in the header.
  {
    direction: 'NorthEast',
    cursor: 'ne-resize',
    style: { top: 0, right: 0, width: CORNER, height: CORNER },
  },
  {
    direction: 'SouthWest',
    cursor: 'sw-resize',
    style: { bottom: 0, left: 0, width: CORNER, height: CORNER },
  },
  {
    direction: 'SouthEast',
    cursor: 'se-resize',
    style: { bottom: 0, right: 0, width: CORNER, height: CORNER },
  },
]

const appWindow = getCurrentWebviewWindow()

const WindowResizeHandles: React.FC = () => {
  const onMouseDown = useCallback(
    (direction: ResizeDirection) => (e: React.MouseEvent) => {
      e.preventDefault()
      e.stopPropagation()
      void appWindow.startResizeDragging(direction).catch(err => {
        console.error('Failed to start window resize:', err)
      })
    },
    [],
  )

  return (
    <>
      {HANDLES.map(({ direction, cursor, style }) => (
        <Box
          key={direction}
          position='fixed'
          zIndex={1000}
          style={{ ...style, cursor }}
          onMouseDown={onMouseDown(direction)}
        />
      ))}
    </>
  )
}

export default WindowResizeHandles
