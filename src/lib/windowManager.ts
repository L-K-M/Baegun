import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window';

export class WindowManager {
  private static readonly TITLE_BAR_HEIGHT = 36;
  private savedWindowSize: { width: number; height: number } | null = null;
  private isShaded = false;
  private appWindow = getCurrentWindow();

  async close(): Promise<void> {
    await this.appWindow.close();
  }

  async startDragging(): Promise<void> {
    await this.appWindow.startDragging();
  }

  async toggleShade(): Promise<boolean> {
    const scaleFactor = await this.appWindow.scaleFactor();

    if (!this.isShaded) {
      const size = await this.appWindow.innerSize();
      const logicalWidth = size.width / scaleFactor;
      const logicalHeight = size.height / scaleFactor;
      this.savedWindowSize = { width: logicalWidth, height: logicalHeight };
      await this.appWindow.setSize(new LogicalSize(logicalWidth, WindowManager.TITLE_BAR_HEIGHT));
      this.isShaded = true;
    } else {
      if (this.savedWindowSize) {
        await this.appWindow.setSize(new LogicalSize(this.savedWindowSize.width, this.savedWindowSize.height));
      }
      this.isShaded = false;
    }

    return this.isShaded;
  }
}
