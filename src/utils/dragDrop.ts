// Допоміжні утиліти для обробки Drag-and-Drop зображень (файли, буфер, браузерні елементи <img>, URL)

export type ExtractedDropImage =
  | { type: "file"; file: File }
  | { type: "data_url"; dataUrl: string; mimeType: string; base64Data: string }
  | { type: "file_path"; path: string }
  | { type: "web_url"; url: string };

export interface DataTransferLike {
  files?: ArrayLike<File>;
  items?: ArrayLike<{
    kind: string;
    type: string;
    getAsFile?: () => File | null;
  }>;
  getData: (format: string) => string;
}

/**
 * Витягує зображення з DataTransfer об'єкта під час події Drop.
 * Підтримує:
 * 1. Локальні File об'єкти (з системного провідника або стільниці).
 * 2. DataTransferItem з типом image/*.
 * 3. HTML-фрагменти (<img src="...">) при перетягуванні фотографій з веб-браузерів (Chrome, Firefox тощо).
 * 4. Списки посилань (text/uri-list, x-special/gnome-copied-files, text/plain) для локальних файлів та веб-зображень.
 */
export function extractImageFromDataTransfer(
  dt: DataTransferLike
): ExtractedDropImage | null {
  // 1. Прямий файл у списку files
  if (dt.files && dt.files.length > 0) {
    for (let i = 0; i < dt.files.length; i++) {
      const file = dt.files[i];
      if (
        (file.type && file.type.startsWith("image/")) ||
        /\.(png|jpe?g|webp|gif|bmp|svg)$/i.test(file.name)
      ) {
        return { type: "file", file };
      }
    }
  }

  // 2. Файл у списку items
  if (dt.items && dt.items.length > 0) {
    for (let i = 0; i < dt.items.length; i++) {
      const item = dt.items[i];
      if (item.kind === "file" && item.type.startsWith("image/")) {
        const file = item.getAsFile?.();
        if (file) {
          return { type: "file", file };
        }
      }
    }
  }

  // 3. HTML-вміст (найбільш надійне джерело при drag-and-drop з зовнішнього браузера)
  const html = dt.getData("text/html");
  if (html) {
    const imgMatch = /<img[^>]+src=["']([^"']+)["']/i.exec(html);
    if (imgMatch && imgMatch[1]) {
      const src = imgMatch[1].trim().replace(/&amp;/g, "&");
      if (src.startsWith("data:image/")) {
        const commaIdx = src.indexOf(",");
        if (commaIdx > -1) {
          const mime = src.slice(5, src.indexOf(";"));
          const base64Data = src.slice(commaIdx + 1);
          return { type: "data_url", dataUrl: src, mimeType: mime, base64Data };
        }
      }
      if (src.startsWith("file://")) {
        let clean = src.replace(/^file:\/\//, "");
        try {
          clean = decodeURIComponent(clean);
        } catch {}
        return { type: "file_path", path: clean };
      }
      if (src.startsWith("http://") || src.startsWith("https://")) {
        return { type: "web_url", url: src };
      }
    }
  }

  // 4. Списки URI (text/uri-list, x-special/gnome-copied-files, text/plain)
  const uriList = dt.getData("text/uri-list") || "";
  const gnomeFiles = dt.getData("x-special/gnome-copied-files") || "";
  const plainText = dt.getData("text/plain") || "";
  const combined = `${uriList}\n${gnomeFiles}\n${plainText}`.trim();

  if (combined) {
    const lines = combined.split(/[\r\n]+/);
    for (const rawLine of lines) {
      let line = rawLine.trim();
      if (!line || line.startsWith("#")) continue;

      if (line.startsWith("copy") || line.startsWith("cut")) {
        line = line.replace(/^(copy|cut)\s*/, "").trim();
      }

      if (line.startsWith("data:image/")) {
        const commaIdx = line.indexOf(",");
        if (commaIdx > -1) {
          const mime = line.slice(5, line.indexOf(";"));
          const base64Data = line.slice(commaIdx + 1);
          return { type: "data_url", dataUrl: line, mimeType: mime, base64Data };
        }
      }

      if (line.startsWith("file://") || line.startsWith("/")) {
        let clean = line.replace(/^file:\/\//, "");
        try {
          clean = decodeURIComponent(clean);
        } catch {}
        if (/\.(png|jpe?g|webp|gif|bmp|svg)$/i.test(clean)) {
          return { type: "file_path", path: clean };
        }
      }

      if (line.startsWith("http://") || line.startsWith("https://")) {
        if (
          /\.(png|jpe?g|webp|gif|bmp|svg)(\?.*)?$/i.test(line) ||
          /\/images?\/|\/img\//i.test(line)
        ) {
          return { type: "web_url", url: line };
        }
      }
    }
  }

  return null;
}

