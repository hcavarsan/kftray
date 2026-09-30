import { ConfirmDialog } from '@/components/ui/dialog'

interface DeleteConfigDialogProps {
  isPending: boolean
  onConfirm: () => void
  onClose: () => void
}

export function DeleteConfigDialog({
  isPending,
  onConfirm,
  onClose,
}: DeleteConfigDialogProps) {
  return (
    <ConfirmDialog
      title='Delete Configuration'
      description="Are you sure? You can't undo this action afterwards."
      isPending={isPending}
      onConfirm={onConfirm}
      onClose={onClose}
    />
  )
}
