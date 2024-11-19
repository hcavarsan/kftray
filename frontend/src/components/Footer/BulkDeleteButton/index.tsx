import React, { useState } from 'react'
import { MdDelete } from 'react-icons/md'

import {
  Box,
  Button,
  DialogBody,
  DialogCloseTrigger,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogRoot,
  DialogTitle,
  Text,
} from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/tauri'

import { useCustomToast } from '@/components/ui/toaster'
import { Tooltip } from '@/components/ui/tooltip'
import { BulkDeleteButtonProps } from '@/types'

const BulkDeleteButton: React.FC<BulkDeleteButtonProps> = ({
  selectedConfigs,
  setSelectedConfigs,
}) => {
  const [state, setState] = useState({
    configsToDelete: [] as number[],
    isDialogOpen: false,
  })
  const toast = useCustomToast()

  const handleOpenChange = (details: { open: boolean }) => {
    setState(prev => ({ ...prev, isDialogOpen: details.open }))
  }

  const handleDeleteClick = (selectedIds: number[]) => {
    setState(prev => ({
      ...prev,
      configsToDelete: selectedIds,
      isDialogOpen: true,
    }))
  }

  const handleConfirmDelete = async () => {
    if (!state.configsToDelete.length) {
      toast({
        title: 'Error',
        description: 'No configurations selected for deletion.',
        status: 'error',
      })

      return
    }

    try {
      await invoke('delete_configs_cmd', { ids: state.configsToDelete })
      setSelectedConfigs([])
      setState(prev => ({ ...prev, isDialogOpen: false }))
      toast({
        title: 'Success',
        description: 'Configurations deleted successfully.',
        status: 'success',
      })
    } catch (error) {
      console.error('Failed to delete configurations:', error)
      toast({
        title: 'Error',
        description: 'Failed to delete configurations.',
        status: 'error',
      })
    }
  }

  if (!selectedConfigs.length) {
    return null
  }

  return (
    <Box>
      <DialogRoot
        role='alertdialog'
        open={state.isDialogOpen}
        onOpenChange={handleOpenChange}
      >
        <Tooltip content='Delete Selected Configs'>
          <Button
            size='2xs'
            variant='ghost'
            onClick={() => handleDeleteClick(selectedConfigs.map(config => config.id))}
            className="delete-button"
          >
            <Box as={MdDelete} width='12px' height='12px' />
          </Button>
        </Tooltip>

        <DialogContent className="delete-dialog">
          <DialogHeader>
            <DialogTitle fontSize='11px' fontWeight='bold'>
              Delete Config(s)
            </DialogTitle>
          </DialogHeader>

          <DialogBody fontSize='11px' py={4}>
            Are you sure you want to delete the selected config(s)? This action cannot be undone.
          </DialogBody>

          <DialogFooter>
            <DialogCloseTrigger asChild>
              <Button
                size='2xs'
                variant='ghost'
                className="cancel-button"
              >
                <Text fontSize='11px'>Cancel</Text>
              </Button>
            </DialogCloseTrigger>
            <Button
              size='2xs'
              variant='ghost'
              onClick={handleConfirmDelete}
              className="confirm-delete-button"
            >
              <Text fontSize='11px'>Delete</Text>
            </Button>
          </DialogFooter>
        </DialogContent>
      </DialogRoot>
    </Box>
  )
}

export default BulkDeleteButton
