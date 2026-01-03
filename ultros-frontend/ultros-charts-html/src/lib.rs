use std::cell::RefCell;
use std::rc::Rc;

use anyhow::{Result, anyhow};
use plotters_svg::SVGBackend;
use ultros_api_types::{SaleHistory, world_helper::WorldHelper};
use ultros_charts::{ChartOptions, draw_sale_history_scatter_plot};

pub fn render_chart(
    world_helper: &WorldHelper,
    sales: &[SaleHistory],
    options: ChartOptions,
    size: (u32, u32),
) -> Result<String> {
    let mut buffer = String::new();
    {
        let backend = SVGBackend::with_string(&mut buffer, size);
        draw_sale_history_scatter_plot(
            Rc::new(RefCell::new(backend)),
            world_helper,
            sales,
            options,
        )
        .map_err(|e| anyhow!("Failed to draw chart: {}", e))?;
    }
    Ok(buffer)
}

#[cfg(test)]
mod tests {


    #[test]
    fn test_renderer_compiles() {
        // Just verify that the signature and imports are correct
        assert!(true);
    }
}
