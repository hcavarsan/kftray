import { useMemo } from 'react'

import { Config, ConfigsByGroup } from '../../types'

export const getConfigGroup = (config: Config): string =>
  config.groups?.trim() || 'Ungrouped'

export const useConfigsByGroup = (filteredConfigs: Config[]): ConfigsByGroup => {
  return useMemo(() => {
    const groupByGroups = (configs: Config[]): ConfigsByGroup => {
      return configs.reduce((group: ConfigsByGroup, config: Config) => {
        const groupKey = getConfigGroup(config)

        if (!group[groupKey]) {
          group[groupKey] = []
        }
        group[groupKey].push(config)

        return group
      }, {})
    }

    const grouped: ConfigsByGroup = groupByGroups(filteredConfigs)
    const sortedKeys = Object.keys(grouped).sort((a, b) => a.localeCompare(b))
    const sortedGroup: ConfigsByGroup = {}

    sortedKeys.forEach(key => {
      const sortedStatuses = [...grouped[key]].sort((a, b) =>
        a.alias.localeCompare(b.alias, undefined, { sensitivity: 'base' }),
      )

      sortedGroup[key] = sortedStatuses
    })

    return sortedGroup
  }, [filteredConfigs])
}
