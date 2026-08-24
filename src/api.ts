import { invoke } from "@tauri-apps/api/core";
import { convertFileSrc } from "@tauri-apps/api/core";
import type {
  AppConfig,
  AnalyzeResult,
  AuditEntry,
  BurstGroup,
  Camera,
  Card,
  CopyTaskReport,
  DeliveryManifest,
  DeviceRegistry,
  EncoderInfo,
  FinalVideoName,
  JobKind,
  Manifest,
  MediaItem,
  PackageInput,
  Project,
  ProjectSummary,
  TranscodeJob,
  TranscodeQueue,
  Volume,
} from "./types";

// ---- 配置 ----
export const getConfig = () => invoke<AppConfig>("get_config");
export const saveConfig = (nas_root: string, operator: string) =>
  invoke<AppConfig>("save_config", { nasRoot: nas_root, operator });

// ---- 项目 ----
export const listProjects = () => invoke<ProjectSummary[]>("list_projects");
export const createProject = (
  date: string,
  name: string,
  workflow: string,
  categories: string[]
) => invoke<Project>("create_project", { date, name, workflow, categories });
export const getProject = (projectId: string) =>
  invoke<Project>("get_project", { projectId });
export const getProjectSummary = (projectId: string) =>
  invoke<ProjectSummary>("get_project_summary", { projectId });

// ---- 设备 ----
export const listDevices = (projectId: string) =>
  invoke<DeviceRegistry>("list_devices", { projectId });
export const addCamera = (
  projectId: string,
  model: string,
  position: string,
  operator: string,
  note?: string
) => invoke<Camera>("add_camera", { projectId, model, position, operator, note });
export const addCard = (
  projectId: string,
  label: string,
  cameraId: string,
  note?: string
) => invoke<Card>("add_card", { projectId, label, cameraId, note });

// ---- 卷 ----
export const scanVolumes = () => invoke<Volume[]>("scan_volumes");

// ---- 拷卡 ----
export const runCopy = (
  projectId: string,
  sourceVolume: string,
  destRelPaths: string[],
  cardLabel: string,
  cameraCode: string,
  note: string,
  resume: boolean
) =>
  invoke<CopyTaskReport>("run_copy", {
    projectId,
    sourceVolume,
    destRelPaths,
    cardLabel,
    cameraCode,
    note,
    resume,
  });
export const listManifests = (projectId: string) =>
  invoke<Manifest[]>("list_manifests", { projectId });

// ---- 媒体 ----
export const scanMediaFolder = (projectId: string, folderRel: string) =>
  invoke<MediaItem[]>("scan_media_folder", { projectId, folderRel });
export const makeThumbnail = (projectId: string, filePath: string) =>
  invoke<string>("make_thumbnail", { projectId, filePath });
export const analyzeFolder = (projectId: string, folderRel: string) =>
  invoke<AnalyzeResult[]>("analyze_folder", { projectId, folderRel });
export const burstGroups = (projectId: string, folderRel: string) =>
  invoke<BurstGroup[]>("burst_groups", { projectId, folderRel });
export const moveToTrash = (projectId: string, filePath: string) =>
  invoke<boolean>("move_to_trash", { projectId, filePath });
export const restoreFromTrash = (projectId: string, filePath: string) =>
  invoke<boolean>("restore_from_trash", { projectId, filePath });
export const moveToCategory = (
  projectId: string,
  filePath: string,
  category: string,
  copy: boolean
) => invoke<boolean>("move_to_category", { projectId, filePath, category, copy });

// ---- 命名 ----
export const validateFinalName = (name: string) =>
  invoke<FinalVideoName>("validate_final_name", { name });

// ---- 转码 ----
export const probeFfmpeg = () => invoke<EncoderInfo>("probe_ffmpeg");
export const listTranscodeJobs = (projectId: string) =>
  invoke<TranscodeQueue>("list_transcode_jobs", { projectId });
export const addTranscodeJob = (
  projectId: string,
  inputPath: string,
  outputRelDir: string,
  kind: JobKind
) => invoke<TranscodeJob>("add_transcode_job", { projectId, inputPath, outputRelDir, kind });
export const runTranscodeJob = (projectId: string, jobId: string) =>
  invoke<TranscodeJob>("run_transcode_job", { projectId, jobId });

// ---- 交付 ----
export const listPackageInputs = (projectId: string, folderRel: string) =>
  invoke<PackageInput[]>("list_package_inputs", { projectId, folderRel });
export const runPackaging = (projectId: string, folderRel: string) =>
  invoke<DeliveryManifest>("run_packaging", { projectId, folderRel });
export const uploadList = (projectId: string) =>
  invoke<string>("upload_list", { projectId });

// ---- 日志 ----
export const getAuditLog = (projectId: string) =>
  invoke<AuditEntry[]>("get_audit_log", { projectId });

// ---- 本地文件转可展示 URL ----
export const toFileUrl = (path: string) => convertFileSrc(path);

export function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}
