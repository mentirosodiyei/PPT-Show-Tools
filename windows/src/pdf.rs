//! PDF pages rendered to PNG with the PDF renderer built into Windows
//! (Windows.Data.Pdf). Rotation and crop box are applied by Windows.
use crate::Result;
use std::path::Path;

#[cfg(windows)]
mod imp {
    use crate::{fail, Result};
    use std::path::Path;
    use windows::core::HSTRING;
    use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
    use windows::Storage::StorageFile;
    use windows::Storage::Streams::{DataReader, InMemoryRandomAccessStream};

    pub struct PdfDoc {
        doc: PdfDocument,
    }
    fn unreadable<T>(_: T) -> crate::Error {
        crate::Error::Fail("The PDF cannot be read, is locked or has no pages.".into())
    }
    impl PdfDoc {
        pub fn open(path: &Path) -> Result<PdfDoc> {
            unsafe {
                let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
            }
            let absolute = std::path::absolute(path).or_else(|e| fail(e.to_string()))?;
            let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(absolute.as_os_str())).map_err(unreadable)?.get().map_err(unreadable)?;
            let doc = PdfDocument::LoadFromFileAsync(&file).map_err(unreadable)?.get().map_err(unreadable)?;
            Ok(PdfDoc { doc })
        }
        pub fn page_count(&self) -> usize {
            self.doc.PageCount().unwrap_or(0) as usize
        }
        /// Page size in points, after rotation.
        pub fn page_size(&self, index: usize) -> Result<(f64, f64)> {
            let page = self.doc.GetPage(index as u32).or_else(|_| fail(format!("Cannot read PDF page {}.", index + 1)))?;
            let size = page.Size().or_else(|_| fail(format!("Cannot read PDF page {}.", index + 1)))?;
            // Windows reports DIPs (1/96 inch).
            Ok((size.Width as f64 * 72.0 / 96.0, size.Height as f64 * 72.0 / 96.0))
        }
        pub fn render_png(&self, index: usize, long_side: u32) -> Result<Vec<u8>> {
            let err = |_| crate::Error::Fail(format!("Cannot render the image of page {}.", index + 1));
            let (w, h) = self.page_size(index)?;
            let scale = long_side as f64 / w.max(h);
            let page = self.doc.GetPage(index as u32).map_err(err)?;
            let options = PdfPageRenderOptions::new().map_err(err)?;
            options.SetDestinationWidth(((w * scale).round() as u32).max(1)).map_err(err)?;
            options.SetDestinationHeight(((h * scale).round() as u32).max(1)).map_err(err)?;
            let stream = InMemoryRandomAccessStream::new().map_err(err)?;
            page.RenderWithOptionsToStreamAsync(&stream, &options).map_err(err)?.get().map_err(err)?;
            let size = stream.Size().map_err(err)? as u32;
            let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0).map_err(err)?).map_err(err)?;
            reader.LoadAsync(size).map_err(err)?.get().map_err(err)?;
            let mut data = vec![0u8; size as usize];
            reader.ReadBytes(&mut data).map_err(err)?;
            if !data.starts_with(b"\x89PNG") {
                return fail(format!("Cannot render the image of page {}.", index + 1));
            }
            Ok(data)
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use crate::{fail, Result};
    use std::path::Path;
    pub struct PdfDoc;
    impl PdfDoc {
        pub fn open(_: &Path) -> Result<PdfDoc> {
            fail("This build converts PDFs with the Windows renderer. On a Mac, use Deckwright for macOS.")
        }
        pub fn page_count(&self) -> usize {
            0
        }
        pub fn page_size(&self, _: usize) -> Result<(f64, f64)> {
            fail("Not available")
        }
        pub fn render_png(&self, _: usize, _: u32) -> Result<Vec<u8>> {
            fail("Not available")
        }
    }
}

pub struct PdfDoc(imp::PdfDoc);
impl PdfDoc {
    pub fn open(path: &Path) -> Result<PdfDoc> {
        imp::PdfDoc::open(path).map(PdfDoc)
    }
    pub fn page_count(&self) -> usize {
        self.0.page_count()
    }
    pub fn page_size(&self, i: usize) -> Result<(f64, f64)> {
        self.0.page_size(i)
    }
    pub fn render_png(&self, i: usize, long_side: u32) -> Result<Vec<u8>> {
        self.0.render_png(i, long_side)
    }
}
