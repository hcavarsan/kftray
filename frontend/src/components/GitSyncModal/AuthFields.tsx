import type { ChangeEvent } from 'react'

import { Input, Stack, Text } from '@chakra-ui/react'

import { Radio, RadioGroup } from '@/components/ui/radio'
import type { AuthMethod } from '@/types'

interface AuthFieldsProps {
  authMethod: AuthMethod
  gitToken: string
  onAuthMethodChange: (details: { value: string | null }) => void
  onGitTokenChange: (value: string) => void
}

export function AuthFields({
  authMethod,
  gitToken,
  onAuthMethodChange,
  onGitTokenChange,
}: AuthFieldsProps) {
  return (
    <Stack gap={2}>
      <Text fontSize='xs' color='fg.muted'>
        Authentication Method
      </Text>
      <Stack
        direction='row'
        gap={2}
        bg='bg.surface'
        p={2}
        borderRadius='md'
        border='1px solid'
        borderColor='border'
      >
        <RadioGroup
          value={authMethod}
          onValueChange={onAuthMethodChange}
          size='xs'
        >
          <Stack direction='row' gap={2}>
            <Radio value='none'>
              <Text fontSize='xs' color='fg.muted'>
                Public Repository
              </Text>
            </Radio>
            <Radio value='system'>
              <Text fontSize='xs' color='fg.muted'>
                Use System Git Credentials
              </Text>
            </Radio>
            <Radio value='token'>
              <Text fontSize='xs' color='fg.muted'>
                GitHub Token
              </Text>
            </Radio>
          </Stack>
        </RadioGroup>
      </Stack>

      {authMethod === 'token' && (
        <Input
          type='password'
          value={gitToken}
          onChange={(e: ChangeEvent<HTMLInputElement>) =>
            onGitTokenChange(e.target.value)
          }
          placeholder='Enter your GitHub token'
          bg='bg.surface'
          borderColor='border'
          _hover={{
            borderColor: 'border.strong',
            bg: 'bg.surface',
          }}
          height='30px'
          fontSize='12px'
        />
      )}
    </Stack>
  )
}
