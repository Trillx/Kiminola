export interface UpdatePreparation {
  flush: () => Promise<void>;
  prepare: () => Promise<void>;
  install: () => Promise<void>;
  cancel: () => Promise<void>;
}

export async function installWhenSaved(steps: UpdatePreparation): Promise<void> {
  await steps.flush();
  try {
    await steps.prepare();
    await steps.install();
  } catch (error) {
    try {
      await steps.cancel();
    } catch (cancelError) {
      throw new Error(`${String(error)}. Could not resume the app: ${String(cancelError)}`);
    }
    throw error;
  }
}
