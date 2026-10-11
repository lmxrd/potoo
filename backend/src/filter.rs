use std::collections::BTreeMap;

/// The RSQL the stats endpoints accept: equality comparisons of the
/// facets the dashboard shows, OR (`,`) within a facet and AND (`;`)
/// across facets. Anything outside that subset is an error.
///
/// ```text
/// country==NO,country==DE;path==/pricing
/// ```
///
/// Each facet, as the column its values are stored in.
const COLUMNS: [&str; 7] = [
    "path", "country", "device", "browser", "os", "referrer", "name",
];

/// The filters of one stats request: facet to values, OR within a facet.
pub type FacetFilters = BTreeMap<String, Vec<String>>;

/// Parse an RSQL filter into the facets it selects, or an error message
/// fit for a 400.
pub fn parse_filter(rsql: &str) -> Result<FacetFilters, String> {
    let mut input = Input::new(rsql);
    let mut groups: Vec<Vec<(String, String)>> = Vec::new();
    loop {
        // One AND term: a run of OR'd comparisons, or a single one.
        let mut group = Vec::new();
        group.push(input.comparison()?);
        while input.comma() {
            group.push(input.comparison()?);
        }
        groups.push(group);
        if !input.semi() {
            break;
        }
    }
    input.end()?;

    let mut filters: FacetFilters = BTreeMap::new();
    for group in groups {
        // OR must stay within one facet: the dashboard cannot express
        // anything else.
        let mut facets: Vec<&str> = group.iter().map(|(f, _)| f.as_str()).collect();
        facets.sort_unstable();
        facets.dedup();
        if facets.len() > 1 {
            return Err("Invalid filter: OR is only supported within a single facet".into());
        }
        for (facet, value) in group {
            if !COLUMNS.contains(&facet.as_str()) {
                return Err(format!("Invalid filter: unknown facet '{facet}'"));
            }
            let values = filters.entry(facet).or_default();
            if !values.contains(&value) {
                values.push(value);
            }
        }
    }
    Ok(filters)
}

/// The filters as a SQL clause over the hits table, with its parameters.
pub fn filter_sql(filters: &FacetFilters) -> (String, Vec<String>) {
    let mut clauses = Vec::new();
    let mut params = Vec::new();
    for (facet, values) in filters {
        let placeholders = values.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        clauses.push(format!("{facet} IN ({placeholders})"));
        params.extend(values.iter().cloned());
    }
    (clauses.join(" AND "), params)
}

/// The filter of a request's query param, or the error to answer with.
pub fn parse_filter_param(raw: Option<&str>) -> Result<FacetFilters, String> {
    match raw {
        None | Some("") => Ok(FacetFilters::new()),
        Some(raw) => parse_filter(raw),
    }
}

/// A character scanner over the filter.
struct Input {
    chars: Vec<char>,
    pos: usize,
}

impl Input {
    fn new(s: &str) -> Self {
        Input {
            chars: s.chars().collect(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_whitespace()) {
            self.pos += 1;
        }
    }

    /// One `facet==value` comparison.
    fn comparison(&mut self) -> Result<(String, String), String> {
        self.skip_ws();
        let facet = self.ident().ok_or_else(|| "Invalid filter".to_string())?;
        self.skip_ws();
        self.operator()?;
        self.skip_ws();
        let value = self.value().ok_or_else(|| "Invalid filter".to_string())?;
        Ok((facet, value))
    }

    /// The selector: letters only, lowercased.
    fn ident(&mut self) -> Option<String> {
        let start = self.pos;
        while matches!(self.peek(), Some(c) if c.is_alphanumeric() || c == '_') {
            self.pos += 1;
        }
        if self.pos == start {
            return None;
        }
        Some(
            self.chars[start..self.pos]
                .iter()
                .collect::<String>()
                .to_lowercase(),
        )
    }

    /// Only `==` is supported; anything else names its operator.
    fn operator(&mut self) -> Result<(), String> {
        let mut op = String::new();
        while matches!(self.peek(), Some('=' | '!' | '<' | '>' | '*')) {
            op.push(self.peek().unwrap());
            self.pos += 1;
        }
        match op.as_str() {
            "==" => Ok(()),
            "" => Err("Invalid filter".into()),
            _ => Err(format!(
                "Invalid filter: the {op} operator is not supported"
            )),
        }
    }

    /// An unquoted run, or a quoted string.
    fn value(&mut self) -> Option<String> {
        match self.peek() {
            Some(quote @ ('\'' | '"')) => {
                self.pos += 1;
                let mut value = String::new();
                loop {
                    match self.peek() {
                        None => return None,
                        Some('\\') => {
                            self.pos += 1;
                            if let Some(c) = self.peek() {
                                value.push(c);
                                self.pos += 1;
                            }
                        }
                        Some(c) if c == quote => {
                            self.pos += 1;
                            return Some(value);
                        }
                        Some(c) => {
                            value.push(c);
                            self.pos += 1;
                        }
                    }
                }
            }
            _ => {
                let start = self.pos;
                while !matches!(self.peek(), None | Some(';' | ','))
                    && !matches!(self.peek(), Some(c) if c.is_whitespace())
                {
                    self.pos += 1;
                }
                if self.pos == start {
                    return None;
                }
                Some(self.chars[start..self.pos].iter().collect())
            }
        }
    }

    /// A consumed `,`, or None.
    fn comma(&mut self) -> bool {
        self.skip_ws();
        if self.peek() == Some(',') {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// A consumed `;`, or None.
    fn semi(&mut self) -> bool {
        self.skip_ws();
        if self.peek() == Some(';') {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// Nothing but whitespace may remain.
    fn end(&mut self) -> Result<(), String> {
        self.skip_ws();
        if self.pos == self.chars.len() {
            Ok(())
        } else {
            Err("Invalid filter".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_or_within_a_facet_and_and_across() {
        let filters = parse_filter("country==NO,country==DE;path==/pricing").unwrap();
        assert_eq!(
            filters,
            FacetFilters::from([
                ("country".into(), vec!["NO".into(), "DE".into()]),
                ("path".into(), vec!["/pricing".into()]),
            ])
        );
    }

    #[test]
    fn parses_quoted_values() {
        let filters = parse_filter("path==\"/with space\"").unwrap();
        assert_eq!(
            filters,
            FacetFilters::from([("path".into(), vec!["/with space".into()])])
        );
    }

    #[test]
    fn deduplicates_values() {
        let filters = parse_filter("device==mobile,device==mobile").unwrap();
        assert_eq!(
            filters,
            FacetFilters::from([("device".into(), vec!["mobile".into()])])
        );
    }

    #[test]
    fn rejects_the_unsupported() {
        assert!(parse_filter("path!=/a").unwrap_err().contains("!="));
        assert!(
            parse_filter("nope==x")
                .unwrap_err()
                .contains("unknown facet")
        );
        assert!(
            parse_filter("path==/a,country==NO")
                .unwrap_err()
                .contains("single facet")
        );
        assert!(parse_filter("path==").is_err());
        assert!(parse_filter("==x").is_err());
        assert!(parse_filter("path==/a,").is_err());
    }

    #[test]
    fn builds_the_sql_clause() {
        let filters = parse_filter("country==NO;device==mobile,device==desktop").unwrap();
        let (clause, params) = filter_sql(&filters);
        assert_eq!(clause, "country IN (?) AND device IN (?, ?)");
        assert_eq!(params, vec!["NO", "mobile", "desktop"]);
    }
}
