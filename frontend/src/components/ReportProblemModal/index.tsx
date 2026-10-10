import { type FormEvent, type ReactNode, useRef, useState } from 'react'
import { TriangleAlert } from 'lucide-react'

import {
  Box,
  Field,
  Flex,
  Input,
  Stack,
  Text,
  Textarea,
} from '@chakra-ui/react'
import { queryOptions, useMutation, useQuery } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import { Button } from '@/components/ui/button'
import {
  AppDialog,
  AppDialogBody,
  AppDialogFooter,
  DialogCancelButton,
} from '@/components/ui/dialog'
import { toaster } from '@/components/ui/toaster'
import {
  type ProblemReportContext,
  problemReportAvailabilityQuery,
  submitProblemReport,
} from '@/lib/problemReports'

const MAX_MESSAGE_LENGTH = 4000
const MAX_EMAIL_LENGTH = 254
const EMAIL_PATTERN = /^[^\s@]+@[^\s@]+\.[^\s@]+$/

const buildInfoQuery = queryOptions({
  queryKey: ['problem-report-build'],
  queryFn: () =>
    invoke<{ release: string; target: string }>('get_telemetry_context'),
  staleTime: Infinity,
})

const fieldStyle = {
  bg: 'bg.surface',
  border: '1px solid',
  borderColor: 'border',
  fontSize: '13px',
  _focus: { borderColor: 'accent.focusRing', boxShadow: 'none' },
  _hover: { borderColor: 'border.emphasized' },
}

function Notice({ children, accent }: { children: ReactNode; accent: string }) {
  return (
    <Flex
      gap={2}
      align='flex-start'
      px={2}
      py={1.5}
      bg='bg.surface'
      border='1px solid'
      borderColor='border'
      borderLeftWidth='2px'
      borderLeftColor={accent}
      borderRadius='md'
    >
      <Box
        as={TriangleAlert}
        width='12px'
        height='12px'
        mt='2px'
        flexShrink={0}
        color={accent}
        aria-hidden
      />
      <Stack gap={1} fontSize='xs' color='fg.secondary' minWidth={0}>
        {children}
      </Stack>
    </Flex>
  )
}

export function ReportProblemModal({
  context,
  onClose,
}: {
  context: ProblemReportContext
  onClose: () => void
}) {
  const [reportId] = useState(() => crypto.randomUUID())
  const [message, setMessage] = useState('')
  const [email, setEmail] = useState('')
  const descriptionRef = useRef<HTMLTextAreaElement>(null)
  const submitting = useRef(false)
  const availability = useQuery(problemReportAvailabilityQuery)
  const build = useQuery(buildInfoQuery)

  const trimmedMessage = message.trim()
  const trimmedEmail = email.trim()
  const messageLength = [...trimmedMessage].length

  const messageError =
    messageLength > MAX_MESSAGE_LENGTH
      ? `Use a description of ${MAX_MESSAGE_LENGTH} characters or fewer.`
      : undefined
  let emailError: string | undefined
  if ([...trimmedEmail].length > MAX_EMAIL_LENGTH) {
    emailError = `Use an email address of ${MAX_EMAIL_LENGTH} characters or fewer.`
  } else if (trimmedEmail && !EMAIL_PATTERN.test(trimmedEmail)) {
    emailError = 'Enter a valid email address or leave it empty.'
  }

  const send = useMutation({
    mutationFn: () =>
      submitProblemReport({
        reportId,
        message: trimmedMessage,
        email: trimmedEmail || undefined,
        context,
      }),
    onSuccess: () => {
      toaster.success({
        title: 'Report submitted',
        duration: 4000,
      })
      onClose()
    },
    onSettled: () => {
      submitting.current = false
    },
    meta: {
      errorToast: {
        title: 'Failed to submit report',
        duration: 6000,
        report: false,
      },
    },
  })

  const reportingAvailable = availability.data === true
  const canSend =
    reportingAvailable &&
    messageLength > 0 &&
    !messageError &&
    !emailError &&
    !send.isPending

  const submit = () => {
    if (canSend && !submitting.current) {
      submitting.current = true
      send.mutate()
    }
  }

  const close = () => {
    if (!submitting.current) {
      onClose()
    }
  }

  const disclosure: [string, string][] = [
    ['Email', trimmedEmail ? 'The address you entered' : 'Not provided'],
    ['App version', build.data?.release || 'Unknown'],
    ['Linked error', context ? 'The error you reported' : 'None'],
  ]

  return (
    <AppDialog
      title='Report a problem'
      onClose={close}
      closeDisabled={send.isPending}
      closeOnInteractOutside={false}
      initialFocusEl={() => descriptionRef.current}
      placement='center'
      maxWidth='420px'
      height='min(620px, 92vh)'
    >
      <Box asChild display='flex' flexDirection='column' flex='1' minHeight={0}>
        <form
          noValidate
          onSubmit={(event: FormEvent) => {
            event.preventDefault()
            submit()
          }}
        >
          <AppDialogBody>
            {availability.data === false || availability.isError ? (
              <Notice accent='warning.border'>
                <Text fontWeight='500' color='fg'>
                  {availability.isError
                    ? 'Could not check whether reporting is available'
                    : 'Problem reports are turned off'}
                </Text>
                <Text color='fg.muted'>
                  {availability.isError
                    ? 'Close this dialog and try again. If it keeps failing, restart kftray.'
                    : 'Development builds and sessions started with the DO_NOT_TRACK environment variable never send reports. Unset it, or use a release build, then restart kftray.'}
                </Text>
                {availability.isError && (
                  <Box>
                    <Button
                      size='2xs'
                      variant='ghost'
                      color='accent.fg'
                      px={1}
                      loading={availability.isFetching}
                      onClick={() => availability.refetch()}
                    >
                      Check again
                    </Button>
                  </Box>
                )}
              </Notice>
            ) : (
              <Stack gap={3}>
                <Field.Root required invalid={!!messageError} gap={1.5}>
                  <Field.Label
                    fontSize='xs'
                    fontWeight='normal'
                    color='fg.muted'
                    m={0}
                  >
                    Description
                    <Field.RequiredIndicator color='fg.subtle' />
                  </Field.Label>
                  <Field.HelperText asChild>
                    <Flex
                      gap={2}
                      align='flex-start'
                      px={2}
                      py={1.5}
                      bg='bg.surface'
                      border='1px solid'
                      borderColor='border'
                      borderLeftWidth='2px'
                      borderLeftColor='warning.border'
                      borderRadius='md'
                      fontSize='xs'
                      color='fg.secondary'
                    >
                      <Box
                        as={TriangleAlert}
                        width='12px'
                        height='12px'
                        mt='2px'
                        flexShrink={0}
                        color='warning.fg'
                        aria-hidden
                      />
                      <Text>
                        Do not include passwords, tokens, kubeconfig contents or
                        cluster, namespace and service names.
                      </Text>
                    </Flex>
                  </Field.HelperText>
                  <Textarea
                    ref={descriptionRef}
                    {...fieldStyle}
                    rows={5}
                    minHeight='96px'
                    resize='vertical'
                    placeholder='What happened, and what did you expect?'
                    value={message}
                    readOnly={send.isPending}
                    onChange={event => setMessage(event.target.value)}
                  />
                  <Flex align='flex-start' gap={2} width='100%'>
                    <Field.ErrorText fontSize='xs' color='danger.fg'>
                      {messageError}
                    </Field.ErrorText>
                    <Text
                      ml='auto'
                      fontSize='11px'
                      flexShrink={0}
                      color={messageError ? 'danger.fg' : 'fg.subtle'}
                    >
                      {messageLength} / {MAX_MESSAGE_LENGTH}
                    </Text>
                  </Flex>
                </Field.Root>

                <Field.Root invalid={!!emailError} gap={1.5}>
                  <Field.Label
                    fontSize='xs'
                    fontWeight='normal'
                    color='fg.muted'
                    m={0}
                  >
                    Email (optional)
                  </Field.Label>
                  <Input
                    {...fieldStyle}
                    height='28px'
                    type='email'
                    autoComplete='email'
                    spellCheck={false}
                    placeholder='Only if you want a reply'
                    value={email}
                    readOnly={send.isPending}
                    onChange={event => setEmail(event.target.value)}
                  />
                  <Field.ErrorText fontSize='xs' color='danger.fg'>
                    {emailError}
                  </Field.ErrorText>
                </Field.Root>

                <Box layerStyle='card' px={2} py={1.5}>
                  <Text fontSize='xs' fontWeight='500' color='fg' mb={1}>
                    Sent with this report
                  </Text>
                  <Box
                    as='dl'
                    display='grid'
                    gridTemplateColumns='auto 1fr'
                    columnGap={3}
                    rowGap={0.5}
                    fontSize='11px'
                    m={0}
                  >
                    {disclosure.map(([label, value]) => (
                      <Box key={label} display='contents'>
                        <Box as='dt' color='fg.subtle'>
                          {label}
                        </Box>
                        <Box
                          as='dd'
                          m={0}
                          color='fg.secondary'
                          wordBreak='break-word'
                        >
                          {value}
                        </Box>
                      </Box>
                    ))}
                  </Box>
                </Box>

                <Text fontSize='11px' color='fg.muted'>
                  Your diagnostics settings stay as they are.
                </Text>

                <Text fontSize='11px' color='fg.subtle'>
                  Sending applies to this report only. Your diagnostics settings
                  stay as they are.
                </Text>
              </Stack>
            )}
          </AppDialogBody>
          <AppDialogFooter>
            <DialogCancelButton onClick={close} disabled={send.isPending} />
            <Button
              type='submit'
              size='xs'
              height='28px'
              bg='accent.solid'
              color='fg'
              _hover={{ bg: 'accent.solidHover' }}
              _active={{ bg: 'accent.solidActive' }}
              disabled={!canSend}
              loading={send.isPending}
              loadingText='Sending...'
            >
              Send report
            </Button>
          </AppDialogFooter>
        </form>
      </Box>
    </AppDialog>
  )
}
