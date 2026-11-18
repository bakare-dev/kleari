import { parsePreview, type CleanupPreview } from "../contracts/cleanup";
import { parseTaskInfo, parseTaskOutcome, type Task, type TaskInfo, type TaskOutcome } from "../contracts/maintenance";
import { call } from "./storageIpc";

/** What each task concerns, measured read-only. */
export function getMaintenanceInfo(): Promise<TaskInfo[]> {
  return call("get_maintenance_info", {}, parseTaskInfo);
}

/** Runs a task you confirmed; macOS asks for an administrator's password for most. */
export function runMaintenanceTask(task: Task): Promise<TaskOutcome> {
  return call("run_maintenance_task", { task }, parseTaskOutcome);
}

/** Prepares your launch agent with this launchd label for removal here: its preview. */
export function prepareLaunchAgentRemoval(label: string): Promise<CleanupPreview> {
  return call("prepare_launch_agent_removal", { label }, parsePreview);
}
