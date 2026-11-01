import { Grid } from '@chakra-ui/react'

import type { GcpBastionProvider, SshProvider } from '@/types'

import { TextField } from '../../Field'

import type { SshProviderFormProps, SshProviderUi } from './index'

// Same rules and messages as GcpBastionProvider::validate in kftray-commons.
const PROJECT_PATTERN = /^[a-z][a-z0-9.:-]{0,99}$/
const ZONE_PATTERN = /^[a-z]+-[a-z]+[0-9]+-[a-z]$/
const INSTANCE_PATTERN = /^[a-z](?:[a-z0-9-]{0,61}[a-z0-9])?$/

const normalize = (provider: SshProvider): SshProvider => ({
  ...provider,
  instance: provider.instance.trim(),
  project: provider.project.trim(),
  zone: provider.zone.trim(),
})

function validate(provider: SshProvider): Record<string, string> {
  const { instance, project, zone } = normalize(provider)
  const errors: Record<string, string> = {}

  if (!project) {
    errors.project = 'Project is required'
  } else if (!PROJECT_PATTERN.test(project)) {
    errors.project = `GCP project "${project}" is not valid (example: my-project-123)`
  }
  if (!zone) {
    errors.zone = 'Zone is required'
  } else if (!ZONE_PATTERN.test(zone)) {
    errors.zone = `GCP zone "${zone}" is not valid (example: us-central1-b)`
  }
  if (!instance) {
    errors.instance = 'Instance is required'
  } else if (!INSTANCE_PATTERN.test(instance)) {
    errors.instance = `GCP instance "${instance}" is not valid (example: jumper)`
  }
  return errors
}

function GcpBastionForm({ provider, errors, onChange }: SshProviderFormProps) {
  return (
    <Grid templateColumns='repeat(3, 1fr)' gap={3}>
      <TextField
        data-testid='config-ssh-gcp-bastion-project'
        error={errors.project}
        label='Project *'
        name='ssh_gcp_project'
        onChange={project => onChange({ ...provider, project })}
        placeholder='my-project'
        value={provider.project}
      />
      <TextField
        data-testid='config-ssh-gcp-bastion-zone'
        error={errors.zone}
        label='Zone *'
        name='ssh_gcp_zone'
        onChange={zone => onChange({ ...provider, zone })}
        placeholder='us-central1-b'
        value={provider.zone}
      />
      <TextField
        data-testid='config-ssh-gcp-bastion-instance'
        error={errors.instance}
        label='Instance *'
        name='ssh_gcp_instance'
        onChange={instance => onChange({ ...provider, instance })}
        placeholder='jumper'
        value={provider.instance}
      />
    </Grid>
  )
}

export const gcpBastion: SshProviderUi = {
  label: 'GCP Bastion (IAP)',
  badge: 'GCP Bastion',
  empty: (): GcpBastionProvider => ({
    type: 'gcp-bastion',
    project: '',
    zone: '',
    instance: '',
  }),
  normalize,
  Form: GcpBastionForm,
  validate,
  summary: ({ instance, project, zone }) => `${instance} (${project}/${zone})`,
}
