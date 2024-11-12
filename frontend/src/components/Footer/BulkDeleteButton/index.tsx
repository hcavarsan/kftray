import React, { useState } from 'react'
import { MdDelete } from 'react-icons/md'

import {
  Box,
  Button,
  DialogActionTrigger,
  DialogBody,
  DialogCloseTrigger,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogRoot,
  DialogTitle,
  DialogTrigger,
  IconButton,
} from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/tauri'

import { useCustomToast } from '@/components/ui/toaster'
import { Tooltip } from '@/components/ui/tooltip'
import { BulkDeleteButtonProps } from '@/types'

const BulkDeleteButton: React.FC<BulkDeleteButtonProps> = ({
  selectedConfigs,
  setSelectedConfigs,
}) => {
  const [configsToDelete, setConfigsToDelete] = useState<number[]>([])
  const toast = useCustomToast()

  const handleDeleteConfigs = (selectedIds: number[]) => {
    setConfigsToDelete(selectedIds)
  }

  const confirmDeleteConfigs = async () => {
    if (!Array.isArray(configsToDelete) || !configsToDelete.length) {
      toast({
        title: 'Error',
        description: 'No configurations selected for deletion.',
        status: 'error',
      })

      return
    }

    try {
      await invoke('delete_configs_cmd', { ids: configsToDelete })
      setSelectedConfigs([])
      toast({
        title: 'Success',
        description: 'Configurations deleted successfully.',
        status: 'success',
      })
    } catch (error) {
      console.error('Failed to delete configurations:', error)
      toast({
        title: 'Error',
        description: 'Failed to delete configurations: "unknown error"',
        status: 'error',
      })
    }
  }

  return (
    <Box>
      {selectedConfigs.length > 0 && (
        <DialogRoot role='alertdialog'>
          <DialogTrigger asChild>
            <Tooltip content='Delete Configs'>
              <IconButton
                aria-label='Delete selected configs'
                colorPalette='red'
                variant='outline'
                onClick={() =>
                  handleDeleteConfigs(selectedConfigs.map(config => config.id))
                }
                size='sm'
              >
                <MdDelete />
              </IconButton>
            </Tooltip>
          </DialogTrigger>

          <DialogContent>
            <DialogHeader>
              <DialogTitle fontSize='xs' fontWeight='bold'>
                Delete Config(s)
              </DialogTitle>
            </DialogHeader>

            <DialogBody fontSize='xs'>
              Are you sure you want to delete the selected config(s)? This
              action cannot be undone.
            </DialogBody>

            <DialogFooter>
              <DialogActionTrigger asChild>
                <Button size='xs'>Cancel</Button>
              </DialogActionTrigger>
              <Button
                colorScheme='red'
                onClick={confirmDeleteConfigs}
                ml={3}
                size='xs'
              >
                Delete
              </Button>
            </DialogFooter>
            <DialogCloseTrigger />
          </DialogContent>
        </DialogRoot>
      )}
    </Box>
  )
}

export default BulkDeleteButton
