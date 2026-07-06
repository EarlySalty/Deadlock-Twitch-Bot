import type { JSX } from "react";
import { MdBlocks } from "../components/Md";
import { CtaLink, Shell } from "../components/Shell";
import { loadFaqGroups } from "../lib/knowledge";

export function FaqPage(): JSX.Element {
  const groups = loadFaqGroups();
  return (
    <Shell>
      <section className="page-head">
        <div className="page-head-art" aria-hidden="true" />
        <div className="container stagger">
          <p className="overline">Fragen &amp; Antworten</p>
          <h1>
            Kurz und <span className="gold">ehrlich beantwortet.</span>
          </h1>
        </div>
      </section>
      <section className="section" style={{ paddingTop: "0" }}>
        <div className="container">
          <nav className="faq-toc reveal" aria-label="FAQ-Themen">
            {groups.map((group) => (
              <a key={group.key} href={`#${group.key}`}>
                {group.title}
              </a>
            ))}
          </nav>

          {groups.map((group) => (
            <div key={group.key} className="faq-group" id={group.key}>
              <h2 className="reveal">{group.title}</h2>
              {group.intro.length > 0 && <MdBlocks blocks={group.intro} />}
              {group.items.map((item) => (
                <details key={item.question} className="panel faq-item">
                  <summary>{item.question}</summary>
                  <MdBlocks blocks={item.blocks} />
                </details>
              ))}
            </div>
          ))}

          <div className="final-inner">
            <div className="hero-actions reveal">
              <CtaLink>Alles klar — Bot reinholen</CtaLink>
            </div>
          </div>
        </div>
      </section>
    </Shell>
  );
}
