import { parseSentAgent, parseTaskInfo, parseTaskOutcome, type Task, type TaskInfo, type TaskOutcome } from "../contracts/maintenance";
import { call } from "./storageIpc";

/** What each task concerns, measured read-only. */
export function getMaintenanceInfo(): Promise<TaskInfo[]> {
  return call("get_maintenance_info", {}, parseTaskInfo);
}

/** Runs a task you confirmed; macOS asks for an administrator's password for most. */
export function runMaintenanceTask(task: Task): Promise<TaskOutcome> {
  return call("run_maintenance_task", { task }, parseTaskOutcome);
}

/** Sends your launch agent with this launchd label to Cleanup's review; nothing moves. */
export function sendLaunchAgentToReview(label: string): Promise<boolean> {
  return call("send_launch_agent_to_review", { label }, parseSentAgent);
}
