/** Never connect (or close a save-only form) before credentials are saved. */
export async function saveThenConnect(
  save: () => void | Promise<void>,
  connect: () => void | Promise<void>,
) {
  await save();
  await connect();
}
