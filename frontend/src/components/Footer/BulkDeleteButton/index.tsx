import React, { useState } from 'react'
import { Trash2 } from 'lucide-react'
import { createPortal } from 'react-dom'

import { Box, Button, HStack, Text } from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/core'

import { toaster } from '@/components/ui/toaster'
import { Tooltip } from '@/components/ui/tooltip'
import { BulkDeleteButtonProps } from '@/types'

const DeleteDialog = ({
  isOpen,
  onClose,
  onConfirm,
}: {
  isOpen: boolean
  onClose: () => void
  onConfirm: () => void
}) => {
  if (!isOpen) {
    return null
  }

  return createPortal(
    <Box
      position='fixed'
      top={0}
      left={0}
      right={0}
      bottom={0}
      zIndex={30}
      display='flex'
      alignItems='center'
      justifyContent='center'
    >
      <Box
        position='fixed'
        top={0}
        left={0}
        right={0}
        bottom={0}
        bg='rgba(0, 0, 0, 0.4)'
        backdropFilter='blur(4px)'
        onClick={onClose}
      />
      <Box
        position='relative'
        maxWidth='400px'
        width='90vw'
        bg='#111111'
        borderRadius='lg'
        border='1px solid rgba(255, 255, 255, 0.08)'
        zIndex={31}
      >
        <Box
          p={1.5}
          bg='#161616'
          borderBottom='1px solid rgba(255, 255, 255, 0.05)'
        >
          <Text fontSize='sm' fontWeight='medium' color='gray.100'>
            Delete Config(s)
          </Text>
        </Box>

        <Box p={3}>
          <Text fontSize='xs' color='gray.400'>
            Are you sure you want to delete the selected config(s)? This action
            cannot be undone.
          </Text>
        </Box>

        <Box p={3} borderTop='1px solid rgba(255, 255, 255, 0.05)' bg='#111111'>
          <HStack justify='flex-end' gap={2}>
            <Button
              size='xs'
              variant='ghost'
              onClick={onClose}
              _hover={{ bg: 'whiteAlpha.50' }}
              height='28px'
            >
              Cancel
            </Button>
            <Button
              size='xs'
              bg='blue.500'
              _hover={{ bg: 'blue.600' }}
              onClick={onConfirm}
              height='28px'
            >
              Delete
            </Button>
          </HStack>
        </Box>
      </Box>
    </Box>,
    document.body,
  )
}

const BulkDeleteButton: React.FC<BulkDeleteButtonProps> = ({
  selectedConfigs,
  setSelectedConfigs,
}) => {
  const [state, setState] = useState({
    configsToDelete: [] as number[],
    isDialogOpen: false,
  })

  const handleDeleteClick = (selectedIds: number[]) => {
    setState(prev => ({
      ...prev,
      configsToDelete: selectedIds,
      isDialogOpen: true,
    }))
  }

  const handleClose = () => {
    setState(prev => ({ ...prev, isDialogOpen: false }))
  }

  const handleConfirmDelete = async () => {
    if (!state.configsToDelete.length) {
      toaster.error({
        title: 'Error',
        description: 'No configurations selected for deletion.',
        duration: 1000,
      })

      return
    }

    try {
      await invoke('delete_configs_cmd', { ids: state.configsToDelete })
      setSelectedConfigs([])
      setState(prev => ({ ...prev, isDialogOpen: false }))
      toaster.success({
        title: 'Success',
        description: 'Configurations deleted successfully.',
        duration: 1000,
      })
    } catch (error) {
      console.error('Failed to delete configurations:', error)
      toaster.error({
        title: 'Error',
        description: 'Failed to delete configurations.',
        duration: 1000,
      })
    }
  }

  if (!selectedConfigs.length) {
    return null
  }

  return (
    <Box>
      <Tooltip
        content='Delete Selected Configs'
        portalled
        positioning={{
          strategy: 'absolute',
          placement: 'top-end',
          offset: { mainAxis: 8, crossAxis: 0 },
        }}
      >
        <Button
          size='sm'
          variant='ghost'
          onClick={() =>
            handleDeleteClick(selectedConfigs.map(config => config.id))
          }
          height='32px'
          minWidth='32px'
          bg='red.500'
          px={1.5}
          borderRadius='md'
          border='1px solid rgba(255, 255, 255, 0.08)'
          _hover={{ bg: 'red.600' }}
        >
          <Box as={Trash2} width='12px' height='12px' />
        </Button>
      </Tooltip>

      <DeleteDialog
        isOpen={state.isDialogOpen}
        onClose={handleClose}
        onConfirm={handleConfirmDelete}
      />
    </Box>
  )
}

export default BulkDeleteButton
