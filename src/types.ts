// 与 Rust 侧 struct 对应的类型定义

export interface AppConfig {
  nas_root: string;
  operator: string;
  machine_id: string;
  default_camera_code?: string | null;
}

export interface Project {
  id: string;
  name: string;
  date: string;
  workflow: "A" | "B";
  categories: string[];
  created_at: string;
}

export interface ProjectSummary {
  id: string;
  name: string;
  workflow: string;
  copied_files: number;
  copied_bytes: number;
  classified: number;
  unclassified: number;
  backup_ok: boolean;
  packaged: boolean;
}

export interface Camera {
  id: string;
  model: string;
  position: string;
  operator: string;
  code: string;
  note?: string | null;
}

export interface Card {
  id: string;
  label: string;
  camera_id: string;
  note?: string | null;
}

export interface DeviceRegistry {
  cameras: Camera[];
  cards: Card[];
}

export interface Volume {
  name: string;
  mount_point: string;
  device?: string | null;
  total_bytes: number;
  available_bytes: number;
}

export interface ManifestFileEntry {
  path: string;
  size: number;
  hash: string;
  verified: boolean;
}

export interface Manifest {
  version: number;
  hash_algo: string;
  project_id: string;
  card_label: string;
  camera_code: string;
  operator: string;
  note: string;
  source_volume: string;
  destinations: string[];
  started_at: string;
  completed_at?: string | null;
  files: ManifestFileEntry[];
}

export interface CopyFileOutcome {
  rel_path: string;
  size: number;
  hash: string;
  verified: boolean;
  skipped: boolean;
  errors: string[];
}

export interface CopyTaskReport {
  manifest: Manifest;
  outcomes: CopyFileOutcome[];
  copied_count: number;
  verified_count: number;
  failed_count: number;
  skipped_count: number;
  total_bytes: number;
  duration_secs: number;
  all_verified: boolean;
}

export interface MediaItem {
  path: string;
  name: string;
  ext: string;
  size: number;
  modified: string;
  datetime_original: string;
  camera_make?: string | null;
  camera_model?: string | null;
  is_video: boolean;
}

export interface QualityScore {
  sharpness: number;
  exposure: number;
  overall: number;
}

export interface AnalyzeResult {
  path: string;
  quality?: QualityScore | null;
  blurry: boolean;
  overexposed: boolean;
  underexposed: boolean;
  eyes_closed?: boolean | null;
  datetime?: string | null;
}

export interface BurstGroup {
  id: number;
  members: string[];
  keep: string[];
}

export interface EncoderInfo {
  platform: string;
  nvenc: boolean;
  qsv: boolean;
  amf: boolean;
  videotoolbox: boolean;
  vaapi: boolean;
  ffmpeg_available: boolean;
}

export type QualityPreset = "high" | "balanced" | "compact";

export type JobKind =
  | { type: "proxy"; preset: QualityPreset }
  | { type: "archive"; preset: QualityPreset };

export type JobStatus = "pending" | "running" | "done" | "failed";

export interface TranscodeJob {
  id: string;
  input: string;
  output_dir: string;
  kind: JobKind;
  status: JobStatus;
  command?: string[] | null;
  error?: string | null;
}

export interface TranscodeQueue {
  jobs: TranscodeJob[];
}

export interface PackageInput {
  path: string;
  name: string;
  size: number;
  datetime?: string | null;
}

export interface Package {
  name: string;
  files: string[];
  count: number;
  total_bytes: number;
}

export interface DeliveryManifest {
  project_id: string;
  created_at: string;
  operator: string;
  packages: Package[];
  total_files: number;
  total_bytes: number;
}

export interface AuditEntry {
  timestamp: string;
  machine_id: string;
  operator: string;
  action: string;
  target: string;
  detail?: string | null;
}

export interface FinalVideoName {
  date: string;
  title: string;
  resolution: string;
  purpose: string;
  version: string;
  is_preview: boolean;
}
