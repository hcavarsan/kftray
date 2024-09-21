import { useMemo } from 'react'

import { Config, ConfigsByContext } from '../../types'

export const useConfigsByContext = (
  filteredConfigs: Config[],
): ConfigsByContext => {
  return useMemo(() => {
    const groupByContext = (configs: Config[]): ConfigsByContext => {
      return configs.reduce((group: ConfigsByContext, config: Config) => {
        const { context } = config

        if (!group[context]) {
          group[context] = []
        }
        group[context].push(config)

        return group
      }, {})
    }

    const grouped: ConfigsByContext = groupByContext(filteredConfigs)
    const sortedKeys = Object.keys(grouped).sort((a, b) => a.localeCompare(b))
    const sortedGroup: ConfigsByContext = {}

    sortedKeys.forEach(key => {
      const sortedStatuses = [...grouped[key]].sort((a, b) =>
        a.alias.localeCompare(b.alias, undefined, { sensitivity: 'base' }),
      )

      sortedGroup[key] = sortedStatuses
    })

    return sortedGroup
  }, [filteredConfigs])
}
