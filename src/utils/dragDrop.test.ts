import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { extractImageFromDataTransfer } from "./dragDrop.ts";

describe("extractImageFromDataTransfer", () => {
  it("extracts direct image file from files array", () => {
    const mockFile = {
      name: "banana.png",
      type: "image/png",
    } as unknown as File;

    const res = extractImageFromDataTransfer({
      files: [mockFile],
      getData: () => "",
    });

    assert.deepEqual(res, { type: "file", file: mockFile });
  });

  it("extracts image from html <img> tag when dragging from browser", () => {
    const htmlSnippet =
      '<a href="https://learn.kotoenglish.com/fruits"><img src="https://learn.kotoenglish.com/wp-content/uploads/2023/10/banana.png" alt="Banana"></a>';

    const res = extractImageFromDataTransfer({
      files: [],
      getData: (format: string) => {
        if (format === "text/html") return htmlSnippet;
        if (format === "text/uri-list")
          return "https://learn.kotoenglish.com/fruits";
        return "";
      },
    });

    assert.deepEqual(res, {
      type: "web_url",
      url: "https://learn.kotoenglish.com/wp-content/uploads/2023/10/banana.png",
    });
  });

  it("extracts base64 data url from <img> src", () => {
    const dataUrl = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUg==";
    const htmlSnippet = `<img src="${dataUrl}">`;

    const res = extractImageFromDataTransfer({
      files: [],
      getData: (format: string) => (format === "text/html" ? htmlSnippet : ""),
    });

    assert.deepEqual(res, {
      type: "data_url",
      dataUrl,
      mimeType: "image/png",
      base64Data: "iVBORw0KGgoAAAANSUhEUg==",
    });
  });

  it("extracts local file path from file:// URI in uri-list", () => {
    const uriList = "file:///home/ivan/Pictures/my%20photo.jpg\r\n";

    const res = extractImageFromDataTransfer({
      files: [],
      getData: (format: string) =>
        format === "text/uri-list" ? uriList : "",
    });

    assert.deepEqual(res, {
      type: "file_path",
      path: "/home/ivan/Pictures/my photo.jpg",
    });
  });

  it("extracts image URL directly from text/uri-list if ends with image extension", () => {
    const uriList = "https://example.com/assets/mascot.webp";

    const res = extractImageFromDataTransfer({
      files: [],
      getData: (format: string) =>
        format === "text/uri-list" ? uriList : "",
    });

    assert.deepEqual(res, {
      type: "web_url",
      url: "https://example.com/assets/mascot.webp",
    });
  });

  it("ignores non-image links and plain text", () => {
    const res = extractImageFromDataTransfer({
      files: [],
      getData: (format: string) =>
        format === "text/uri-list"
          ? "https://google.com/search?q=banana"
          : "some random text",
    });

    assert.equal(res, null);
  });
});
