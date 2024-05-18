import React from 'react'

import { Box, Text, useTheme } from '@chakra-ui/react'

import { CustomToastProps } from '../../types'

const CustomToast: React.FC<CustomToastProps> = ({
  title,
  description,
  status,
}) => {
  const theme = useTheme()

  const statusColorMap: Record<string, string> = {
    info: theme.colors.blue[700],
    warning: theme.colors.orange[700],
    success: theme.colors.green[700],
    error: theme.colors.red[700],
  }

  const backgroundColor = statusColorMap[status] || theme.colors.gray[500]

  return (
    <Box
      color='white'
      p={3}
      bg={backgroundColor}
      fontSize='xs'
      maxWidth='300px'
      borderRadius='lg'
      boxShadow='md'
    >
      <Text fontWeight='bold' fontSize='12px'>
        {title}
      </Text>
      <Text mt={1} fontSize='10px'>
        {description}
      </Text>
    </Box>
  )
}

export default CustomToast
