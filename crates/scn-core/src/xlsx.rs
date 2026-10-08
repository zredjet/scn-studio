//! 表を Excel 形式（.xlsx、Office Open XML）にする（台本の Excel 形式。T2-10、D061）。
//!
//! 依存を足さずに、最小のブックを作る：シートは1枚（宴への書き出しは複数。T3-12）、欄はすべて文字列（インライン文字列。行IDが数や日付に
//! 変わらない）、見出しの行は太字で固定、列の幅は呼び手が決める。ZIP は圧縮しない（stored）。時刻は固定
//! （1980-01-01）にして、同じ表からは同じバイト列を作る。

/// CRC-32（ZIP と同じ多項式 0xEDB88320。PNG も同じ）。
pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// ZIP の書き手（stored だけ。EPUB も使う。T3-13）。
pub(crate) struct Zip {
    out: Vec<u8>,
    central: Vec<u8>,
    count: u16,
}

/// ZIP の大きさの上限（ZIP64 を使わない）を超えた。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("表が大きすぎて Excel 形式にできません（4GB まで）")]
pub struct TooLarge;

impl Zip {
    pub(crate) fn new() -> Self {
        Zip {
            out: Vec::new(),
            central: Vec::new(),
            count: 0,
        }
    }

    pub(crate) fn add(&mut self, name: &str, data: &[u8]) -> Result<(), TooLarge> {
        let crc = crc32(data);
        let size = u32::try_from(data.len()).map_err(|_| TooLarge)?;
        let offset = u32::try_from(self.out.len()).map_err(|_| TooLarge)?;
        let name_len = u16::try_from(name.len()).map_err(|_| TooLarge)?;
        // 版 2.0、フラグなし、stored、時刻 00:00:00、日付 1980-01-01。
        let fixed = |buf: &mut Vec<u8>| {
            buf.extend_from_slice(&20u16.to_le_bytes());
            buf.extend_from_slice(&0u16.to_le_bytes());
            buf.extend_from_slice(&0u16.to_le_bytes());
            buf.extend_from_slice(&0u16.to_le_bytes());
            buf.extend_from_slice(&0x0021u16.to_le_bytes());
            buf.extend_from_slice(&crc.to_le_bytes());
            buf.extend_from_slice(&size.to_le_bytes());
            buf.extend_from_slice(&size.to_le_bytes());
            buf.extend_from_slice(&name_len.to_le_bytes());
            buf.extend_from_slice(&0u16.to_le_bytes());
        };
        self.out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        fixed(&mut self.out);
        self.out.extend_from_slice(name.as_bytes());
        self.out.extend_from_slice(data);

        self.central
            .extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        self.central.extend_from_slice(&20u16.to_le_bytes());
        fixed(&mut self.central);
        // 注釈の長さ、ディスク番号、内部属性、外部属性、ローカルヘッダの位置。
        self.central.extend_from_slice(&0u16.to_le_bytes());
        self.central.extend_from_slice(&0u16.to_le_bytes());
        self.central.extend_from_slice(&0u16.to_le_bytes());
        self.central.extend_from_slice(&0u32.to_le_bytes());
        self.central.extend_from_slice(&offset.to_le_bytes());
        self.central.extend_from_slice(name.as_bytes());
        self.count = self.count.checked_add(1).ok_or(TooLarge)?;
        Ok(())
    }

    pub(crate) fn finish(mut self) -> Result<Vec<u8>, TooLarge> {
        let offset = u32::try_from(self.out.len()).map_err(|_| TooLarge)?;
        let size = u32::try_from(self.central.len()).map_err(|_| TooLarge)?;
        self.out.extend_from_slice(&self.central);
        self.out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        self.out.extend_from_slice(&0u16.to_le_bytes());
        self.out.extend_from_slice(&0u16.to_le_bytes());
        self.out.extend_from_slice(&self.count.to_le_bytes());
        self.out.extend_from_slice(&self.count.to_le_bytes());
        self.out.extend_from_slice(&size.to_le_bytes());
        self.out.extend_from_slice(&offset.to_le_bytes());
        self.out.extend_from_slice(&0u16.to_le_bytes());
        u32::try_from(self.out.len()).map_err(|_| TooLarge)?;
        Ok(self.out)
    }
}

/// XML の文字にする。`&`・`<`・`>`・`"` を実体参照にし、XML 1.0 で書けない制御文字（タブと改行のほか）は
/// 落とす。改行は LF にそろえる（Excel の欄の中の改行）。
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.replace("\r\n", "\n").chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' => out.push(c),
            '\r' => out.push('\n'),
            c if u32::from(c) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

/// 列の名前（0 → A、25 → Z、26 → AA）。
fn column(mut index: usize) -> String {
    let mut name = Vec::new();
    loop {
        let rest = u8::try_from(index % 26).unwrap_or(0);
        name.push(char::from(b'A' + rest));
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    name.iter().rev().collect()
}

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/></Types>"#;

const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;

const WORKBOOK_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#;

/// 書式：0 は標準（上詰め、折り返し）、1 は見出し（太字）。
const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="2"><font><sz val="11"/><name val="Yu Gothic"/></font><font><b/><sz val="11"/><name val="Yu Gothic"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0" applyAlignment="1"><alignment vertical="top" wrapText="1"/></xf><xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"/></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#;

/// シートの XML（見出しの行と、データの行）。
fn sheet_xml(header: &[&str], rows: &[Vec<String>], widths: &[u32]) -> String {
    let mut out = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetViews><sheetView workbookViewId="0"><pane ySplit="1" topLeftCell="A2" activePane="bottomLeft" state="frozen"/></sheetView></sheetViews><sheetFormatPr defaultRowHeight="15"/>"#,
    );
    if !widths.is_empty() {
        out.push_str("<cols>");
        for (i, w) in widths.iter().enumerate() {
            let n = i + 1;
            out.push_str(&format!(
                r#"<col min="{n}" max="{n}" width="{w}" customWidth="1"/>"#
            ));
        }
        out.push_str("</cols>");
    }
    out.push_str("<sheetData>");
    let mut row = |number: usize, cells: &mut dyn Iterator<Item = &str>, style: u8| {
        out.push_str(&format!(r#"<row r="{number}">"#));
        for (i, cell) in cells.enumerate() {
            if cell.is_empty() {
                continue;
            }
            out.push_str(&format!(
                r#"<c r="{}{number}" s="{style}" t="inlineStr"><is><t xml:space="preserve">{}</t></is></c>"#,
                column(i),
                escape(cell)
            ));
        }
        out.push_str("</row>");
    };
    row(1, &mut header.iter().copied(), 1);
    for (i, cells) in rows.iter().enumerate() {
        row(i + 2, &mut cells.iter().map(String::as_str), 0);
    }
    out.push_str("</sheetData></worksheet>");
    out
}

/// ブックの1枚のシート。
pub(crate) struct Sheet<'a> {
    /// シートの名前（31文字まで。`[]:*?/\` は使わない）。
    pub(crate) name: &'a str,
    pub(crate) header: &'a [&'a str],
    pub(crate) rows: &'a [Vec<String>],
    pub(crate) widths: &'a [u32],
}

/// 複数のシートのブックを .xlsx のバイト列にする（宴への書き出し。T3-12）。
pub(crate) fn xlsx_book(sheets: &[Sheet<'_>]) -> Result<Vec<u8>, TooLarge> {
    let mut types = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#,
    );
    let mut rels = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    let mut list = String::new();
    for (i, sheet) in sheets.iter().enumerate() {
        let n = i + 1;
        types.push_str(&format!(
            r#"<Override PartName="/xl/worksheets/sheet{n}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#
        ));
        rels.push_str(&format!(
            r#"<Relationship Id="rId{n}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{n}.xml"/>"#
        ));
        list.push_str(&format!(
            r#"<sheet name="{}" sheetId="{n}" r:id="rId{n}"/>"#,
            escape(sheet.name)
        ));
    }
    types.push_str(r#"<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/></Types>"#);
    rels.push_str(&format!(
        r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#,
        sheets.len() + 1
    ));
    let workbook = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>{list}</sheets></workbook>"#
    );
    let mut zip = Zip::new();
    zip.add("[Content_Types].xml", types.as_bytes())?;
    zip.add("_rels/.rels", ROOT_RELS.as_bytes())?;
    zip.add("xl/workbook.xml", workbook.as_bytes())?;
    zip.add("xl/_rels/workbook.xml.rels", rels.as_bytes())?;
    zip.add("xl/styles.xml", STYLES.as_bytes())?;
    for (i, sheet) in sheets.iter().enumerate() {
        zip.add(
            &format!("xl/worksheets/sheet{}.xml", i + 1),
            sheet_xml(sheet.header, sheet.rows, sheet.widths).as_bytes(),
        )?;
    }
    zip.finish()
}

/// 表を .xlsx のバイト列にする。`sheet` はシートの名前（31文字まで。`[]:*?/\` は使わない）。
pub(crate) fn xlsx_table(
    sheet: &str,
    header: &[&str],
    rows: &[Vec<String>],
    widths: &[u32],
) -> Result<Vec<u8>, TooLarge> {
    let workbook = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="{}" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
        escape(sheet)
    );
    let mut zip = Zip::new();
    zip.add("[Content_Types].xml", CONTENT_TYPES.as_bytes())?;
    zip.add("_rels/.rels", ROOT_RELS.as_bytes())?;
    zip.add("xl/workbook.xml", workbook.as_bytes())?;
    zip.add("xl/_rels/workbook.xml.rels", WORKBOOK_RELS.as_bytes())?;
    zip.add("xl/styles.xml", STYLES.as_bytes())?;
    zip.add(
        "xl/worksheets/sheet1.xml",
        sheet_xml(header, rows, widths).as_bytes(),
    )?;
    zip.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_and_columns() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(
            [0, 6, 25, 26, 27, 701, 702].map(column),
            ["A", "G", "Z", "AA", "AB", "ZZ", "AAA"]
        );
    }

    #[test]
    fn escapes_and_skips_control_characters() {
        assert_eq!(
            escape("a<b>&\"c\"\r\nd\u{1}\u{3000}e"),
            "a&lt;b&gt;&amp;&quot;c&quot;\nd\u{3000}e"
        );
    }

    #[test]
    fn sheet_cells_are_inline_strings() {
        let xml = sheet_xml(
            &["行ID", "台詞"],
            &[
                vec!["0012".to_owned(), " 先頭の空白".to_owned()],
                vec![String::new(), "x".to_owned()],
            ],
            &[10, 40],
        );
        assert!(xml.contains(
            r#"<c r="A1" s="1" t="inlineStr"><is><t xml:space="preserve">行ID</t></is></c>"#
        ));
        assert!(xml.contains(
            r#"<c r="A2" s="0" t="inlineStr"><is><t xml:space="preserve">0012</t></is></c>"#
        ));
        assert!(xml.contains(r#"<t xml:space="preserve"> 先頭の空白</t>"#));
        // 空の欄は書かない。
        assert!(xml.contains(r#"<row r="3"><c r="B3""#));
        assert!(xml.contains(r#"<col min="2" max="2" width="40" customWidth="1"/>"#));
    }
}
