import { useRootRouteLoaderData } from '@/routes/_root';
import { useFileRouteLoaderData } from '@/shared/hooks/useFileRouteLoaderData';

export const useIsAvailableArgs = () => {
  const { isAuthenticated } = useRootRouteLoaderData();
  const {
    userMakingRequest: { fileTeamPrivacy, teamPermissions, filePermissions },
  } = useFileRouteLoaderData();

  const isAvailableArgs = {
    isAuthenticated,
    filePermissions,
    fileTeamPrivacy,
    teamPermissions,
  };

  return isAvailableArgs;
};
