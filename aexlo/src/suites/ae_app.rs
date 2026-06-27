pub(super) fn create_ae_app_suite_v6() -> Box {
	Box::new(PF_AE_AppSuite6 {
		PF_GetAppName: Some(get_app_name_sys),
	})
}
