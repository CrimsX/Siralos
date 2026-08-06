import { createDeterministicFakeProvider } from "@solaris/adapters";
import {
  createSolarisApplication,
  createToolRegistry,
  type SolarisApplication,
} from "@solaris/core";

export interface CliApplication {
  readonly providerId: string;
  readonly application: SolarisApplication;
}

export function createCliApplication(): CliApplication {
  const provider = createDeterministicFakeProvider();
  const application = createSolarisApplication({ provider, tools: createToolRegistry([]) });
  return { providerId: provider.id, application };
}
