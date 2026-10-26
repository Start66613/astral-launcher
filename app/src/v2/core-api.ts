import type { Task } from "../lib/types";
import { invokeLive } from "./live-api";

// V2 adapters mirror e0ebb7c's registered commands. Shared legacy DTOs stay intact.
export type LoaderKind = "fabric" | "quilt" | "forge" | "neoforge";
export interface LoaderBuild {
  id: string;
  kind: LoaderKind;
  version: string;
  mcVersion: string;
  stability: string;
  recommended: boolean;
  publishedAt: string | null;
  available: boolean;
  unavailableReason: string | null;
}
export interface Issue {
  code: string;
  message: string;
  blocking: boolean;
}
export interface Plan {
  planId: string;
  revision: number;
  expiresAt: string;
  issues: Issue[];
  downloadBytes: number | null;
}
export interface ComponentBuild {
  id: string;
  label: string;
  projectId: string;
  requiredLoader: string | null;
  versionId: string | null;
  version: string | null;
  fileName: string | null;
  publishedAt: string | null;
  stability: string;
  recommended: boolean;
  available: boolean;
  unavailableReason: string | null;
}
export interface ComponentVersion {
  versionId: string;
  versionNumber: string;
  channel: string;
  gameVersions: string[];
  loaders: string[];
  fileName: string;
  size: number;
  publishedAt: string;
  recommended: boolean;
}
export interface GameRequest {
  name: string;
  gameVersion: string;
  loader?: { kind: LoaderKind; version: string } | null;
  memoryMb?: number;
  shaderSupport?: string;
  shaderSupportVersion?: string;
}
export interface GamePlan extends Plan {
  request: GameRequest;
  target: {
    proposedInstanceId: string;
    displayPath: string;
    isolated: boolean;
  };
  files: {
    id: string;
    displayName: string;
    sizeBytes: number | null;
    role: string;
    action: string;
    relativePath: string;
  }[];
  components: {
    id: string;
    label: string;
    projectId: string;
    versionId: string;
    fileName: string;
    sizeBytes: number;
  }[];
}
export interface ResourceProject {
  id: string;
  title: string;
  description: string;
  iconUrl: string | null;
  downloads: number;
  updatedAt: string | null;
  sourceUrl: string | null;
  licenseId: string | null;
}
export interface ResourceFile {
  versionId: string;
  name: string;
  versionNumber: string;
  channel: string;
  publishedAt: string;
  gameVersions: string[];
  loaders: string[];
  fileName: string;
  url: string;
  size: number;
  sha1: string | null;
  sha512: string | null;
  dependencies: {
    projectId: string | null;
    versionId: string | null;
    kind: string;
  }[];
}
export interface WorldTarget {
  worldId: string;
  displayName: string;
  dir: string;
  lastModified: string;
  sizeBytes: number;
  gameVersion: string | null;
}
export type ResourceKind = "mod" | "resourcepack" | "shaderpack" | "datapack";
export interface ResourceRequest {
  instanceId: string;
  kind: ResourceKind;
  projectId: string;
  versionId: string;
  worldId?: string;
  overwrite: boolean;
}
export interface ResourcePlan extends Plan {
  instanceId: string;
  kind: string;
  projectId: string;
  versionId: string;
  fileName: string;
  targetDir: string;
  replaces: string | null;
  dependencies: {
    projectId: string;
    versionId: string;
    fileName: string;
    size: number;
    sha1: string | null;
    sha512: string | null;
  }[];
}
export interface ModpackRequest {
  projectId: string;
  versionId: string;
  name?: string;
  memoryMb?: number;
}
export interface ModpackPlan extends Plan {
  info: {
    name: string;
    summary: string;
    versionName: string;
    mcVersion: string;
    loaderKind: string | null;
    loaderVersion: string | null;
    fileCount: number;
    totalBytes: number;
  };
}
export interface JavaRuntime {
  path: string;
  version: string;
  major: number;
  vendor: string;
  bits: number | null;
  source: string;
}
export interface JavaReport {
  runtimes: JavaRuntime[];
  requiredMajor: number | null;
  recommended: string | null;
}
export interface ManagedJdk {
  major: number;
  dir: string;
  bytes: number;
  javaPath: string | null;
}
export interface ModUpdate {
  fileName: string;
  projectId: string;
  currentVersion: string;
  latestVersion: string;
  latestVersionId: string;
  latestFileName: string;
}
export interface UpdateReport {
  checked: number;
  unknown: number;
  updates: ModUpdate[];
  skipped: number;
}
export interface LogFile {
  id: string;
  name: string;
  kind: string;
  sizeBytes: number;
  modifiedAt: string;
}
export interface LogText {
  id: string;
  name: string;
  text: string;
  lines: number;
  truncated: boolean;
}
export interface Diagnosis {
  logId: string;
  logName: string;
  lines: number;
  findings: {
    id: string;
    label: string;
    severity: string;
    evidence: string[];
    hint: string;
  }[];
  text: string;
}
export interface TaskV2 extends Task {
  bootId: string;
  cancelling: boolean;
  phase: string;
  stageLabel: string;
  retrySpec: unknown | null;
}
export const listLoaderBuilds = (gameVersion: string, kind: LoaderKind) =>
  invokeLive<LoaderBuild[]>("list_loader_builds", { gameVersion, kind });
export const getComponentCatalog = (gameVersion: string, loader?: string) =>
  invokeLive<{ shaderSupport: ComponentBuild[]; fetchedAt: string }>(
    "get_component_catalog",
    { gameVersion, loader },
  );
export const listComponentVersions = (
  id: string,
  gameVersion: string,
  loader?: string,
) =>
  invokeLive<ComponentVersion[]>("list_component_versions", {
    id,
    gameVersion,
    loader,
  });
export const prepareGameInstall = (request: GameRequest) =>
  invokeLive<GamePlan>("prepare_game_install", { request });
export const startGameInstall = (planId: string, revision: number) =>
  invokeLive<TaskV2>("start_game_install", { planId, revision });
export const getResourceProject = (projectId: string) =>
  invokeLive<ResourceProject>("get_resource_project", { projectId });
export const listResourceFiles = (
  projectId: string,
  gameVersion?: string,
  loader?: string,
) =>
  invokeLive<ResourceFile[]>("list_resource_files", {
    projectId,
    gameVersion,
    loader,
  });
export const listTargetWorlds = (instanceId: string) =>
  invokeLive<WorldTarget[]>("list_target_worlds", { instanceId });
export const prepareResourceInstall = (request: ResourceRequest) =>
  invokeLive<ResourcePlan>("prepare_resource_install", { request });
export const startResourceInstall = (planId: string, revision: number) =>
  invokeLive<TaskV2>("start_resource_install", { planId, revision });
export const prepareModpackInstall = (request: ModpackRequest) =>
  invokeLive<ModpackPlan>("prepare_modpack_install", { request });
export const startModpackInstall = (planId: string, revision: number) =>
  invokeLive<TaskV2>("start_modpack_install", { planId, revision });
export const exportModpack = (request: {
  instanceId: string;
  name: string;
  version?: string;
  includeConfig?: boolean;
}) =>
  invokeLive<{
    path: string;
    indexedFiles: number;
    overrideFiles: number;
    bytes: number;
  }>("export_modpack", { request });
export const checkModUpdates = (instanceId: string, limit = 100) =>
  invokeLive<UpdateReport>("check_mod_updates", { instanceId, limit });
export const startModUpdate = (
  instanceId: string,
  fileName: string,
  projectId: string,
  versionId: string,
) =>
  invokeLive<TaskV2>("start_mod_update", {
    instanceId,
    fileName,
    projectId,
    versionId,
  });
export const listJavaRuntimes = (instanceId?: string) =>
  invokeLive<JavaReport>("list_java_runtimes", { instanceId });
export const listManagedJdks = () =>
  invokeLive<ManagedJdk[]>("list_managed_jdks");
export const installJava = (major: number) =>
  invokeLive<TaskV2>("install_java", { major });
export const repairInstance = (instanceId: string) =>
  invokeLive<TaskV2>("repair_instance", { instanceId });
export const retryTask = (taskId: string) =>
  invokeLive<TaskV2>("retry_task", { taskId });
export const listLogs = (instanceId: string) =>
  invokeLive<LogFile[]>("list_logs", { instanceId });
export const readLog = (instanceId: string, logId: string, limit = 500) =>
  invokeLive<LogText>("read_log", { instanceId, logId, limit });
export const diagnoseInstance = (
  instanceId: string,
  logId?: string,
  maxLines = 500,
) =>
  invokeLive<Diagnosis>("diagnose_instance", { instanceId, logId, maxLines });
export const prepareDiagnosis = (
  instanceId: string,
  logIds: string[],
  maxLines = 500,
) =>
  invokeLive<{
    text: string;
    redactions: number;
    bytes: number;
    truncated: boolean;
    warnings: string[];
  }>("prepare_diagnosis", { instanceId, logIds, maxLines });
export const getMsaStatus = () =>
  invokeLive<{
    configured: boolean;
    clientIdMasked: string | null;
    reason: string | null;
    steps: string[];
  }>("msa_status");
export const getAccount = () =>
  invokeLive<{
    kind: string;
    name: string;
    online: boolean;
    note: string | null;
  }>("get_account");
export function cancelledLate(task: Task) {
  return Boolean(
    (task.result as unknown as { cancelledLate?: boolean } | null)
      ?.cancelledLate,
  );
}
