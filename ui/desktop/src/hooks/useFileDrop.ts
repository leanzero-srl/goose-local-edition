import { useCallback, useState, useRef, useEffect } from 'react';
import { compressImageDataUrl, errorMessage } from '../utils/conversionUtils';
import { mediaKindOf } from '../types/message';
import { arrayBufferToBase64, speechWavDataUrl } from '../utils/speechWav';

/** Largest audio/video file attached inline (it travels base64 in the prompt). */
export const MAX_MEDIA_BYTES = 50 * 1024 * 1024;

/** Read an audio/video file into its DroppedFile entry: dataUrl when done, error when not. */
export function readMediaFile(
  file: File,
  entry: DroppedFile,
  update: (id: string, patch: Partial<DroppedFile>) => void,
  readers?: Set<FileReader>
): void {
  if (file.size > MAX_MEDIA_BYTES) {
    update(entry.id, { isLoading: false, error: 'Over 50 MB' });
    return;
  }
  const reader = new FileReader();
  readers?.add(reader);
  if (entry.mediaKind === 'audio') {
    // Audio goes as 16 kHz mono PCM WAV (utils/speechWav.ts); the file itself when undecodable.
    reader.onload = (event) => {
      readers?.delete(reader);
      const bytes = event.target?.result as ArrayBuffer;
      void speechWavDataUrl(bytes).then((wav) =>
        update(entry.id, {
          dataUrl: wav ?? `data:${file.type || 'audio/wav'};base64,${arrayBufferToBase64(bytes)}`,
          isLoading: false,
        })
      );
    };
    reader.onerror = () => {
      update(entry.id, { isLoading: false, error: 'Failed to read file' });
      readers?.delete(reader);
    };
    reader.onabort = () => readers?.delete(reader);
    reader.readAsArrayBuffer(file);
    return;
  }
  reader.onload = (event) => {
    update(entry.id, { dataUrl: event.target?.result as string, isLoading: false });
    readers?.delete(reader);
  };
  reader.onerror = () => {
    update(entry.id, { isLoading: false, error: 'Failed to read file' });
    readers?.delete(reader);
  };
  reader.onabort = () => readers?.delete(reader);
  reader.readAsDataURL(file);
}

export interface DroppedFile {
  id: string;
  path: string;
  name: string;
  type: string;
  isImage: boolean;
  /** Audio or video: read whole as a data URL and sent to the model as a media part. */
  mediaKind?: 'audio' | 'video';
  dataUrl?: string;
  isLoading?: boolean;
  error?: string;
}

export const useFileDrop = () => {
  const [droppedFiles, setDroppedFiles] = useState<DroppedFile[]>([]);
  const activeReadersRef = useRef<Set<FileReader>>(new Set());

  // Cleanup effect to prevent memory leaks
  useEffect(() => {
    return () => {
      // Abort any active FileReaders on unmount
      // eslint-disable-next-line react-hooks/exhaustive-deps
      const readers = activeReadersRef.current;
      readers.forEach((reader) => {
        try {
          reader.abort();
        } catch {
          // Reader might already be done, ignore errors
        }
      });
      readers.clear();
    };
  }, []);

  const handleDrop = useCallback(async (e: React.DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    const files = e.dataTransfer.files;
    if (files.length > 0) {
      const droppedFileObjects: DroppedFile[] = [];

      for (let i = 0; i < files.length; i++) {
        const file = files[i];

        let droppedFile: DroppedFile;

        try {
          const path = window.electron.getPathForFile(file);
          const isImage = file.type.startsWith('image/');
          const mediaKind = mediaKindOf(file.type) ?? undefined;

          droppedFile = {
            id: `dropped-${Date.now()}-${i}`,
            path,
            name: file.name,
            type: file.type,
            isImage,
            ...(mediaKind ? { mediaKind } : {}),
            // Images and media need a loading state while they are read
            isLoading: isImage || mediaKind !== undefined,
          };
        } catch (error) {
          console.error('Error processing file:', file.name, error);
          // Create an error file object
          droppedFile = {
            id: `dropped-error-${Date.now()}-${i}`,
            path: '',
            name: file.name,
            type: file.type,
            isImage: false,
            isLoading: false,
            error: `Failed to get file path: ${errorMessage(error, 'Unknown error')}`,
          };
        }

        droppedFileObjects.push(droppedFile);

        if (droppedFile.mediaKind && !droppedFile.error) {
          readMediaFile(
            file,
            droppedFile,
            (id, patch) =>
              setDroppedFiles((prev) => prev.map((f) => (f.id === id ? { ...f, ...patch } : f))),
            activeReadersRef.current
          );
        }

        if (droppedFile.isImage && !droppedFile.error) {
          const reader = new FileReader();
          activeReadersRef.current.add(reader);

          reader.onload = async (event) => {
            const dataUrl = event.target?.result as string;
            try {
              // Compress the image
              const compressedDataUrl = await compressImageDataUrl(dataUrl);
              setDroppedFiles((prev) =>
                prev.map((f) =>
                  f.id === droppedFile.id
                    ? { ...f, dataUrl: compressedDataUrl, isLoading: false }
                    : f
                )
              );
            } catch (compressionError) {
              console.error('Failed to compress image:', file.name, compressionError);
              setDroppedFiles((prev) =>
                prev.map((f) =>
                  f.id === droppedFile.id
                    ? { ...f, error: 'Failed to compress image', isLoading: false }
                    : f
                )
              );
            }
            activeReadersRef.current.delete(reader);
          };

          reader.onerror = () => {
            console.error('Failed to read image:', file.name);
            setDroppedFiles((prev) =>
              prev.map((f) =>
                f.id === droppedFile.id
                  ? { ...f, error: 'Failed to load image', isLoading: false }
                  : f
              )
            );
            activeReadersRef.current.delete(reader);
          };

          reader.onabort = () => {
            activeReadersRef.current.delete(reader);
          };

          reader.readAsDataURL(file);
        }
      }

      setDroppedFiles((prev) => [...prev, ...droppedFileObjects]);
    }
  }, []);

  const handleDragOver = useCallback((e: React.DragEvent<HTMLDivElement>) => {
    e.preventDefault();
  }, []);

  return {
    droppedFiles,
    setDroppedFiles,
    handleDrop,
    handleDragOver,
  };
};
