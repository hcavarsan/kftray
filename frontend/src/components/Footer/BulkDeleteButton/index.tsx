import { useState } from 'react'
import { Trash2 } from 'lucide-react'

import { Box, Button } from '@chakra-ui/react'

import { ConfirmDialog } from '@/components/ui/dialog'
import { toaster } from '@/components/ui/toaster'
import { Tooltip } from '@/components/ui/tooltip'
import { errorMessage } from '@/lib/errors'
import type { Config } from '@/types'

interface BulkDeleteButtonProps {
  selectedConfigs: Config[]
  deleteConfigs: (ids: number[]) => Promise<boolean>
}

function BulkDeleteButton({
  selectedConfigs,
  deleteConfigs,
}: BulkDeleteButtonProps) {
  const [state, setState] = useState({
    configsToDelete: [] as number[],
    isDialogOpen: false,
    isDeleting: false,
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
    if (state.isDeleting) {
      return
    }
    if (!state.configsToDelete.length) {
      toaster.error({
        title: 'Error',
        description: 'No configurations selected for deletion.',
        duration: 1000,
      })

      return
    }

    setState(prev => ({ ...prev, isDeleting: true }))
    try {
      const deleted = await deleteConfigs(state.configsToDelete)

      if (deleted) {
        setState(prev => ({ ...prev, isDialogOpen: false }))
      }
    } catch (error) {
      toaster.error({
        title: 'Error deleting configs',
        description: errorMessage(error),
        duration: 2000,
      })
    } finally {
      setState(prev => ({ ...prev, isDeleting: false }))
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
          aria-label='Delete selected configs'
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
          border='1px solid'
          borderColor='app.border'
          _hover={{ bg: 'red.600' }}
        >
          <Box as={Trash2} width='12px' height='12px' />
        </Button>
      </Tooltip>

      {state.isDialogOpen && (
        <ConfirmDialog
          title='Delete Config(s)'
          description='Are you sure you want to delete the selected config(s)? This action cannot be undone.'
          isPending={state.isDeleting}
          onConfirm={() => void handleConfirmDelete()}
          onClose={handleClose}
        />
      )}
    </Box>
  )
}

export default BulkDeleteButton
