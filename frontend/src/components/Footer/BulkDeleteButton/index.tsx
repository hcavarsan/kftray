import { useState } from 'react'
import { Trash2 } from 'lucide-react'

import { Box } from '@chakra-ui/react'
import { useMutation } from '@tanstack/react-query'

import { FooterActionButton } from '@/components/Footer/FooterActionButton'
import { ConfirmDialog } from '@/components/ui/dialog'
import { Tooltip } from '@/components/ui/tooltip'
import type { Config } from '@/types'

interface BulkDeleteButtonProps {
  selectedConfigs: Config[]
  deleteConfigs: (ids: number[]) => Promise<boolean>
}

function BulkDeleteButton({
  selectedConfigs,
  deleteConfigs,
}: BulkDeleteButtonProps) {
  const [idsToDelete, setIdsToDelete] = useState<number[] | null>(null)

  const deleteMutation = useMutation({
    mutationFn: deleteConfigs,
    onSuccess: deleted => {
      if (deleted) {
        setIdsToDelete(null)
      }
    },
  })

  if (!selectedConfigs.length) {
    return null
  }

  return (
    <Box>
      <Tooltip
        content='Delete Selected Configs'
        positioning={{
          placement: 'top-end',
          offset: { mainAxis: 8, crossAxis: 0 },
        }}
      >
        <FooterActionButton
          aria-label='Delete selected configs'
          onClick={() =>
            setIdsToDelete(selectedConfigs.map(config => config.id))
          }
          bg='red.500'
          _hover={{ bg: 'red.600' }}
        >
          <Box as={Trash2} width='12px' height='12px' />
        </FooterActionButton>
      </Tooltip>

      {idsToDelete && (
        <ConfirmDialog
          title='Delete Config(s)'
          description='Are you sure you want to delete the selected config(s)? This action cannot be undone.'
          isPending={deleteMutation.isPending}
          onConfirm={() => deleteMutation.mutate(idsToDelete)}
          onClose={() => setIdsToDelete(null)}
        />
      )}
    </Box>
  )
}

export default BulkDeleteButton
