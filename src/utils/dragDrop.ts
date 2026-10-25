// Допоміжні утиліти для обробки Drag-and-Drop зображень (файли, буфер, браузерні елементи <img>, CSS background-image, URL)

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

function cleanCandidateUrl(raw: string): string {
  let cleaned = raw.trim();
  cleaned = cleaned.replace(/&amp;/g, "&");
  cleaned = cleaned.replace(/&quot;/g, "");
  cleaned = cleaned.replace(/&#39;/g, "");
  cleaned = cleaned.replace(/['"]/g, "");
  return cleaned;
}

function urlToExtractedImage(rawUrl: string): ExtractedDropImage | null {
  const url = cleanCandidateUrl(rawUrl);
  if (!url) return null;

  // 1. Data URLs (base64)
  if (url.startsWith("data:image/")) {
    const commaIdx = url.indexOf(",");
    if (commaIdx > -1) {
      const mime = url.slice(5, url.indexOf(";"));
      const base64Data = url.slice(commaIdx + 1);
      return { type: "data_url", dataUrl: url, mimeType: mime, base64Data };
    }
  }

  // 2. Локальні файли (file:// або абсолютні шляхи)
  if (url.startsWith("file://") || url.startsWith("/")) {
    let clean = url.replace(/^file:\/\//, "");
    try {
      clean = decodeURIComponent(clean);
    } catch {}
    if (/\.(png|jpe?g|webp|gif|bmp|svg|avif|ico)$/i.test(clean)) {
      return { type: "file_path", path: clean };
    }
  }

  // 3. Веб-посилання (http:// або https://)
  // Приймаємо будь-який валідний HTTP/HTTPS URL — бекенд самостійно перевірить Content-Type
  // Це дозволяє підтримувати YouTube банери (yt3.googleusercontent.com), Google Images, CDN без розширень тощо.
  if (url.startsWith("http://") || url.startsWith("https://")) {
    return { type: "web_url", url };
  }

  return null;
}

/**
 * Витягує зображення з DataTransfer об'єкта під час події Drop.
 * Підтримує:
 * 1. Локальні File об'єкти (з системного провідника або стільниці).
 * 2. DataTransferItem з типом image/*.
 * 3. HTML-фрагменти:
 *    - <img src="...">
 *    - <tp-yt-iron-image src="..."> (YouTube елементи)
 *    - <div style="background-image: url(...)"> (YouTube банери, обкладинки)
 *    - <source srcset="..."> (Picture елементи)
 *    - <image href="..."> (SVG елементи)
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
        /\.(png|jpe?g|webp|gif|bmp|svg|avif|ico)$/i.test(file.name)
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
    // 3a. Шукаємо src або data-src або srcset на будь-якому тезі (img, iron-image, yt-image, source тощо)
    const srcMatch =
      /<[^>]+?\b(?:src|data-src)=["']([^"']+)["']/i.exec(html) ||
      /<[^>]+?\bsrcset=["']([^"'\s,]+)/i.exec(html);
    if (srcMatch && srcMatch[1]) {
      const res = urlToExtractedImage(srcMatch[1]);
      if (res) return res;
    }

    // 3b. Шукаємо CSS background-image: url(...) (типово для YouTube банерів JerryRigEverything тощо)
    const bgMatch =
      /background(?:-image)?\s*:\s*url\(\s*(?:&quot;|['"])?([^"')&]+)(?:&quot;|['"])?\s*\)/i.exec(
        html
      ) ||
      /url\(\s*(?:&quot;|['"])?(https?:\/\/[^"')&]+)(?:&quot;|['"])?\s*\)/i.exec(
        html
      );
    if (bgMatch && bgMatch[1]) {
      const res = urlToExtractedImage(bgMatch[1]);
      if (res) return res;
    }

    // 3c. Шукаємо SVG <image href="..."> або <image xlink:href="...">
    const svgMatch =
      /<image[^>]+?\b(?:href|xlink:href)=["']([^"']+)["']/i.exec(html);
    if (svgMatch && svgMatch[1]) {
      const res = urlToExtractedImage(svgMatch[1]);
      if (res) return res;
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

      const res = urlToExtractedImage(line);
      if (res) return res;
    }
  }

  return null;
}

export type ExtractedDropTarget =
  | { type: "image"; extractedImage: ExtractedDropImage }
  | { type: "file"; file: File }
  | { type: "file_path"; path: string; name: string }
  | { type: "web_url"; url: string };

/**
 * Універсальний екстрактор для будь-яких типів файлів (відео, аудіо, документів, архівів, зображень)
 * із DataTransfer об'єкта під час події Drop.
 */
export function extractAnyDropTarget(dt: DataTransferLike): ExtractedDropTarget | null {
  // 1. Локальні шляхи у text/uri-list, gnome-copied-files або text/plain (містять точний абсолютний шлях до файлу!)
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
      if (line.startsWith("file://") || line.startsWith("/")) {
        let clean = line.replace(/^file:\/\//, "");
        try {
          clean = decodeURIComponent(clean);
        } catch {}
        const name = clean.split("/").filter(Boolean).pop() || "file";
        return { type: "file_path", path: clean, name };
      }
      if (line.startsWith("http://") || line.startsWith("https://")) {
        return { type: "web_url", url: line };
      }
    }
  }

  // 2. Перевіряємо зображення (зберігаємо прев'ю та vision)
  const img = extractImageFromDataTransfer(dt);
  if (img) {
    return { type: "image", extractedImage: img };
  }

  // 3. Прямі файли у dt.files
  if (dt.files && dt.files.length > 0) {
    return { type: "file", file: dt.files[0] };
  }

  // 4. Файли у dt.items
  if (dt.items && dt.items.length > 0) {
    for (let i = 0; i < dt.items.length; i++) {
      const item = dt.items[i];
      if (item.kind === "file") {
        const file = item.getAsFile?.();
        if (file) return { type: "file", file };
      }
    }
  }

  return null;
}
