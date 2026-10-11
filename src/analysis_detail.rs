//! Shared computational budgets for the viewer and external solver CLI.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Detail {
    Preview,
    Standard,
    Fine,
    Ultra,
}
impl Detail {
    pub fn label(self) -> &'static str {
        match self {
            Self::Preview => "Preview",
            Self::Standard => "Standard",
            Self::Fine => "Fine",
            Self::Ultra => "Ultra",
        }
    }
    pub fn flow(self) -> ([usize; 2], usize) {
        match self {
            Self::Preview => ([96, 64], 1000),
            Self::Standard => ([384, 256], 6000),
            Self::Fine => ([1024, 512], 16000),
            Self::Ultra => ([2048, 1024], 32000),
        }
    }
    pub fn thermal(self) -> crate::thermal::Resolution {
        let (axial_cells, fin_cells, max_step_s) = match self {
            Self::Preview => (8, 2, 0.05),
            Self::Standard => (32, 8, 0.0125),
            Self::Fine => (64, 16, 0.00625),
            Self::Ultra => (128, 32, 0.0015625),
        };
        crate::thermal::Resolution {
            axial_cells,
            fin_cells,
            max_step_s,
        }
    }
    pub fn boundary_step(self) -> f64 {
        match self {
            Self::Preview => 0.2,
            Self::Standard => 0.05,
            Self::Fine => 0.025,
            Self::Ultra => 0.0125,
        }
    }
    pub fn stress(self) -> usize {
        match self {
            Self::Preview => 32,
            Self::Standard => 128,
            Self::Fine => 256,
            Self::Ultra => 512,
        }
    }
    pub fn wall_layers(self) -> usize {
        match self {
            Self::Preview => 2,
            Self::Standard => 8,
            Self::Fine => 16,
            Self::Ultra => 32,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refinement_increases_real_work_and_preserves_gpu_dispatch_limits() {
        let mut previous = (0, 0, 0, 0, 0.3);
        for detail in [
            Detail::Preview,
            Detail::Standard,
            Detail::Fine,
            Detail::Ultra,
        ] {
            let (grid, iterations) = detail.flow();
            let cells = grid[0] * grid[1];
            let heat = detail.thermal();
            assert!(cells > previous.0 && iterations > previous.1);
            assert!(heat.axial_cells > previous.2 && detail.stress() > previous.3);
            assert!(detail.boundary_step() < previous.4);
            assert!(cells.div_ceil(64) <= 65535);
            assert!(cells * 16 <= 128 * 1024 * 1024);
            assert!(3 * (heat.axial_cells + heat.fin_cells) < 1024);
            previous = (
                cells,
                iterations,
                heat.axial_cells,
                detail.stress(),
                detail.boundary_step(),
            );
        }
        assert!(Detail::Ultra.flow().0[0] * Detail::Ultra.flow().0[1] > 10 * 512 * 384);
    }
}
