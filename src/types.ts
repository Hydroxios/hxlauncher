export type Settings = {
  memoryMb: number;
  storageDirectory: string;
  instancesDirectory?: string;
};

export type Instance = {
  id: string;
  name: string;
  version: string;
  loader: string;
  status: string;
  modCount: number;
  iconPath?: string;
};

export type Store = { settings: Settings; instances: Instance[] };

export type Progress = {
  phase: string;
  message: string;
  current: number;
  total: number;
};

export type Pack = {
  name: string;
  version: string;
  author: string;
  minecraft: string;
  loader: string;
  files: { projectID: number; fileID: number; required: boolean }[];
  overrideCount: number;
  archivePath: string;
};

export type Device = {
  userCode: string;
  verificationUri: string;
  expiresIn: number;
  interval: number;
};

export type Page = "library" | "packs" | "settings" | "activity";
export type Notice = { text: string; error: boolean };
export type LogEntry = { time: string; text: string };
