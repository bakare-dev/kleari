import { parseTrashList, parseTrashOutcome, type TrashList, type TrashOutcome } from "../contracts/trash";
import { call } from "./storageIpc";

/** What is in the Trash (yours and each external disk's); needs Full Disk Access. */
export function listTrash(): Promise<TrashList> {
  return call("list_trash", {}, parseTrashList);
}

/** Deletes the chosen items from the Trash for good. */
export function deleteTrashItems(ids: readonly string[]): Promise<TrashOutcome> {
  return call("delete_trash_items", { ids }, parseTrashOutcome);
}

/** Puts the chosen items back where Finder says they came from (never over anything there). */
export function putBackTrashItems(ids: readonly string[]): Promise<TrashOutcome> {
  return call("put_back_trash_items", { ids }, parseTrashOutcome);
}
