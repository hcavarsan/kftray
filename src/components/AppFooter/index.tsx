import React, { useEffect, useState } from 'react'

import { Box, Text } from '@chakra-ui/react'
import { app } from '@tauri-apps/api'

const AppFooter: React.FC = () => {
  const [version, setVersion] = useState('')

  useEffect(() => {
    app.getVersion().then(setVersion)
  }, [])

  return (
    <Box as='footer' width='100%' p='4' textAlign='center'>
      <Text fontSize='sm'>Version: {version}</Text>
    </Box>
  )
}

export default AppFooter
