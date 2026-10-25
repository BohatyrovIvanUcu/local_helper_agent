import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { extractImageFromDataTransfer, extractAnyDropTarget } from "./dragDrop.ts";

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

  it("extracts YouTube channel banner with CSS background-image", () => {
    const bannerUrl =
      "https://yt3.googleusercontent.com/w9U8v5c7b4d1d-JerryRig=w1707-fcrop64=1,00005a57ffffa5a8-k-c0xffffffff-no-nd-rj";
    const htmlSnippet = `<div id="banner" style="background-image: url(&quot;${bannerUrl}&quot;);"></div>`;

    const res = extractImageFromDataTransfer({
      files: [],
      getData: (format: string) => (format === "text/html" ? htmlSnippet : ""),
    });

    assert.deepEqual(res, {
      type: "web_url",
      url: bannerUrl,
    });
  });

  it("extracts YouTube custom element tp-yt-iron-image", () => {
    const bannerUrl =
      "https://yt3.googleusercontent.com/w9U8v5c7b4d1d-JerryRig=w1060";
    const htmlSnippet = `<tp-yt-iron-image src="${bannerUrl}"></tp-yt-iron-image>`;

    const res = extractImageFromDataTransfer({
      files: [],
      getData: (format: string) => (format === "text/html" ? htmlSnippet : ""),
    });

    assert.deepEqual(res, {
      type: "web_url",
      url: bannerUrl,
    });
  });

  it("extracts YouTube CDN URL directly from text/uri-list even without image extension", () => {
    const uriList =
      "https://yt3.googleusercontent.com/w9U8v5c7b4d1d-JerryRig=w1707-fcrop64=1,00005a57ffffa5a8-k-c0xffffffff-no-nd-rj";

    const res = extractImageFromDataTransfer({
      files: [],
      getData: (format: string) =>
        format === "text/uri-list" ? uriList : "",
    });

    assert.deepEqual(res, {
      type: "web_url",
      url: uriList,
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
});

describe("extractAnyDropTarget", () => {
  it("extracts image target when an image is dropped", () => {
    const mockFile = {
      name: "screen.png",
      type: "image/png",
    } as unknown as File;

    const res = extractAnyDropTarget({
      files: [mockFile],
      getData: () => "",
    });

    assert.equal(res?.type, "image");
  });

  it("extracts video file when mp4 is dropped via files array", () => {
    const mockFile = {
      name: "vacation.mp4",
      type: "video/mp4",
    } as unknown as File;

    const res = extractAnyDropTarget({
      files: [mockFile],
      getData: () => "",
    });

    assert.deepEqual(res, {
      type: "file",
      file: mockFile,
    });
  });

  it("extracts file path for non-image files via file:// URI", () => {
    const uriList = "file:///home/ivan/Videos/vacation%20clip.mp4\r\n";

    const res = extractAnyDropTarget({
      files: [],
      getData: (format: string) =>
        format === "text/uri-list" ? uriList : "",
    });

    assert.deepEqual(res, {
      type: "file_path",
      path: "/home/ivan/Videos/vacation clip.mp4",
      name: "vacation clip.mp4",
    });
  });
});
