import { invoke } from '@tauri-apps/api/core';

export const getLogDir = () => invoke<string>('get_log_dir');
export const openLogFolder = () => invoke<void>('open_log_folder');
