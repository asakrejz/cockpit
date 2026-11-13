use leptos::prelude::*;

/// Column in the [`Footer`].
#[derive(Clone, Debug)]
pub struct FooterColumn {
    pub heading: &'static str,
    pub links: Vec<&'static str>,
}

/// Page footer with link columns and legal texts.
#[component]
pub fn Footer(
    columns: Vec<FooterColumn>,
    #[prop(into)] copyright: String,
    #[prop(optional)] legal: Vec<&'static str>,
) -> impl IntoView {
    view! {
        <footer class="ui-footer">
            <div class="ui-container">
                <div class="ui-footer__cols">
                    {columns
                        .into_iter()
                        .map(|col| {
                            view! {
                                <div>
                                    <p class="ui-footer__heading">{col.heading}</p>
                                    <ul>
                                        {col
                                            .links
                                            .into_iter()
                                            .map(|l| view! { <li><a href="#">{l}</a></li> })
                                            .collect_view()}
                                    </ul>
                                </div>
                            }
                        })
                        .collect_view()}
                </div>
                <div class="ui-hstack" style="justify-content: space-between; padding-top: 20px">
                    <span>{copyright}</span>
                    <div class="ui-hstack" style="--spacing: 16px">
                        {legal.into_iter().map(|l| view! { <a href="#">{l}</a> }).collect_view()}
                    </div>
                </div>
            </div>
        </footer>
    }
}
