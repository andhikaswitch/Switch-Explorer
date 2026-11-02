//! gpui-component `DataTable` delegate over a [`Dataset`]. Row presses are recorded in a shared
//! cell so the workspace can start OLE drags and native menus for the pressed row (row handlers
//! run before the container's, in GPUI's bubble order).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::component::table::{Column, TableDelegate, TableState};
use gpui_kit::component::{ActiveTheme as _, h_flex};
use gpui_kit::*;

use crate::data::Dataset;

pub type RowHit = Rc<Cell<Option<usize>>>;

/// Display strings for visible rows; small and reset when it grows.
const CACHE_LIMIT: usize = 2048;

pub struct FileTable {
    pub data: Rc<Dataset>,
    hit: RowHit,
    columns: Vec<Column>,
    cache: RefCell<HashMap<usize, [SharedString; 4]>>,
}

impl FileTable {
    pub fn new(data: Rc<Dataset>, hit: RowHit) -> Self {
        Self {
            data,
            hit,
            columns: vec![
                Column::new("name", "Nama").width(px(440.)),
                Column::new("modified", "Diubah").width(px(150.)),
                Column::new("kind", "Tipe").width(px(110.)),
                Column::new("size", "Ukuran").width(px(100.)).text_right(),
            ],
            cache: RefCell::default(),
        }
    }

    pub fn set_data(&mut self, data: Rc<Dataset>) {
        self.data = data;
        self.cache.borrow_mut().clear();
    }

    fn cells(&self, row: usize) -> [SharedString; 4] {
        let mut cache = self.cache.borrow_mut();
        if cache.len() > CACHE_LIMIT {
            cache.clear();
        }
        cache
            .entry(row)
            .or_insert_with(|| self.data.cells(row))
            .clone()
    }
}

impl TableDelegate for FileTable {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.data.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        self.columns[col_ix].clone()
    }

    fn render_tr(
        &mut self,
        row_ix: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let (left, right) = (self.hit.clone(), self.hit.clone());
        div()
            .id(("row", row_ix))
            .on_mouse_down(MouseButton::Left, move |_, _, _| left.set(Some(row_ix)))
            .on_mouse_down(MouseButton::Right, move |_, _, _| right.set(Some(row_ix)))
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let cells = self.cells(row_ix);
        if col_ix == 0 {
            let color = if self.data.is_dir(row_ix) {
                cx.theme().warning
            } else {
                cx.theme().info
            };
            h_flex()
                .gap_2()
                .overflow_hidden()
                .child(div().size(px(12.)).flex_none().rounded_sm().bg(color))
                .child(cells[0].clone())
                .into_any_element()
        } else {
            cells[col_ix].clone().into_any_element()
        }
    }
}
